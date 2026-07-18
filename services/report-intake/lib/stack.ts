import * as cdk from 'aws-cdk-lib';
import { Construct } from 'constructs';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { NodejsFunction } from 'aws-cdk-lib/aws-lambda-nodejs';
import * as path from 'path';

export interface ReportIntakeStackProps extends cdk.StackProps {
  githubRepo: string;      // "owner/repo" issues are filed against
  ssmTokenParam: string;   // SSM SecureString name holding the fine-grained PAT
  ssmTokensParam: string;  // shared beta invite roster; removing a line revokes capabilities
  ssmCapabilitySecretParam: string;
}

export class ReportIntakeStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: ReportIntakeStackProps) {
    super(scope, id, props);

    // Private beta evidence bucket. Screenshots expire quickly; aggregate check-ins are retained for
    // the length of the validation cycle. GitHub issues receive seven-day signed image links.
    const bucket = new s3.Bucket(this, 'Images', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      encryption: s3.BucketEncryption.S3_MANAGED,
      cors: [{
        allowedMethods: [s3.HttpMethods.POST, s3.HttpMethods.PUT],
        allowedOrigins: ['*'],       // presigned POST is authorized by the signature; CORS just lets the browser send it
        allowedHeaders: ['*'],
        maxAge: 3000,
      }],
      lifecycleRules: [
        { prefix: 'images/', expiration: cdk.Duration.days(30) },
        { prefix: 'checkins/', expiration: cdk.Duration.days(365) },
      ],
      // A mistaken stack deletion must not erase beta evidence ahead of its lifecycle policy.
      removalPolicy: cdk.RemovalPolicy.RETAIN,
    });
    const tokenArn = `arn:aws:ssm:${this.region}:${this.account}:parameter${props.ssmTokenParam}`;
    const tokensArn = `arn:aws:ssm:${this.region}:${this.account}:parameter${props.ssmTokensParam}`;
    const capabilityArn = `arn:aws:ssm:${this.region}:${this.account}:parameter${props.ssmCapabilitySecretParam}`;

    const fn = new NodejsFunction(this, 'Intake', {
      entry: path.join(__dirname, '..', 'lambda', 'handler.ts'),
      runtime: lambda.Runtime.NODEJS_22_X,
      timeout: cdk.Duration.seconds(15),
      memorySize: 256,
      reservedConcurrentExecutions: 5,
      environment: {
        BUCKET: bucket.bucketName,
        GITHUB_REPO: props.githubRepo,
        SSM_TOKEN_PARAM: props.ssmTokenParam,
        SSM_TOKENS_PARAM: props.ssmTokensParam,
        SSM_CAPABILITY_SECRET_PARAM: props.ssmCapabilitySecretParam,
        MAX_IMAGES: '3',
        MAX_IMAGE_MB: '5',
      },
      bundling: { minify: true, target: 'node22' },
    });

    // Least privilege: read only that one SecureString, write only into images/ on this bucket.
    fn.addToRolePolicy(new iam.PolicyStatement({
      actions: ['ssm:GetParameter'],
      resources: [tokenArn, tokensArn, capabilityArn],
    }));
    bucket.grantPut(fn, 'images/*');
    bucket.grantRead(fn, 'images/*');
    bucket.grantPut(fn, 'checkins/*');

    const url = fn.addFunctionUrl({
      authType: lambda.FunctionUrlAuthType.NONE, // capability auth is enforced in the handler
    });

    new cdk.CfnOutput(this, 'IntakeUrl', { value: url.url, description: 'POST here from the dashboard modal (REPORT_ENDPOINT)' });
    new cdk.CfnOutput(this, 'BucketName', { value: bucket.bucketName });
  }
}
