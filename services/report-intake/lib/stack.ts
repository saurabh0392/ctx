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
}

export class ReportIntakeStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: ReportIntakeStackProps) {
    super(scope, id, props);

    // Images bucket. Objects under images/ are public-read (so they render in the issue and persist),
    // but keys are unguessable UUIDs and listing is denied, so a URL reveals one image and nothing more.
    const bucket = new s3.Bucket(this, 'Images', {
      blockPublicAccess: new s3.BlockPublicAccess({
        blockPublicAcls: true,
        ignorePublicAcls: true,
        blockPublicPolicy: false,   // allow the bucket policy below
        restrictPublicBuckets: false,
      }),
      encryption: s3.BucketEncryption.S3_MANAGED,
      cors: [{
        allowedMethods: [s3.HttpMethods.POST, s3.HttpMethods.PUT],
        allowedOrigins: ['*'],       // presigned POST is authorized by the signature; CORS just lets the browser send it
        allowedHeaders: ['*'],
        maxAge: 3000,
      }],
      lifecycleRules: [{ prefix: 'images/', expiration: cdk.Duration.days(90) }],
      removalPolicy: cdk.RemovalPolicy.DESTROY,   // alpha: tear down cleanly with cdk destroy
      autoDeleteObjects: true,
    });
    bucket.addToResourcePolicy(new iam.PolicyStatement({
      actions: ['s3:GetObject'],
      resources: [bucket.arnForObjects('images/*')],
      principals: [new iam.AnyPrincipal()],
    }));

    const tokenArn = `arn:aws:ssm:${this.region}:${this.account}:parameter${props.ssmTokenParam}`;

    const fn = new NodejsFunction(this, 'Intake', {
      entry: path.join(__dirname, '..', 'lambda', 'handler.ts'),
      runtime: lambda.Runtime.NODEJS_20_X,
      timeout: cdk.Duration.seconds(15),
      memorySize: 256,
      environment: {
        BUCKET: bucket.bucketName,
        GITHUB_REPO: props.githubRepo,
        SSM_TOKEN_PARAM: props.ssmTokenParam,
        MAX_IMAGES: '25',
        MAX_IMAGE_MB: '10',
      },
      bundling: { minify: true, target: 'node20' },
    });

    // Least privilege: read only that one SecureString, write only into images/ on this bucket.
    fn.addToRolePolicy(new iam.PolicyStatement({
      actions: ['ssm:GetParameter'],
      resources: [tokenArn],
    }));
    bucket.grantPut(fn, 'images/*');

    const url = fn.addFunctionUrl({
      authType: lambda.FunctionUrlAuthType.NONE,   // public: the modal has no AWS credentials
      cors: {
        allowedOrigins: ['*'],
        allowedMethods: [lambda.HttpMethod.POST],
        allowedHeaders: ['content-type'],
      },
    });

    new cdk.CfnOutput(this, 'IntakeUrl', { value: url.url, description: 'POST here from the dashboard modal (REPORT_ENDPOINT)' });
    new cdk.CfnOutput(this, 'BucketName', { value: bucket.bucketName });
  }
}
