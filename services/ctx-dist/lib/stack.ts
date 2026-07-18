import * as cdk from 'aws-cdk-lib';
import { Construct } from 'constructs';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { NodejsFunction } from 'aws-cdk-lib/aws-lambda-nodejs';
import * as path from 'path';

export interface CtxDistStackProps extends cdk.StackProps {
  // SSM SecureString holding the beta invite roster (one token per line, optional "= label").
  ssmTokensParam: string;
  ssmCapabilitySecretParam: string;
  feedbackEndpoint?: string;
}

export class CtxDistStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: CtxDistStackProps) {
    super(scope, id, props);

    // Private artifact bucket: binaries, checksums, the manifest, and install scripts. Nothing here is
    // public. The Lambda serves install.sh openly and presigns binary downloads only for valid tokens,
    // so a leaked download URL is one binary for five minutes, not the whole bucket.
    const bucket = new s3.Bucket(this, 'Artifacts', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      encryption: s3.BucketEncryption.S3_MANAGED,
      versioned: true,
      removalPolicy: cdk.RemovalPolicy.RETAIN, // keep shipped binaries even if the stack is torn down
    });

    const tokensArn = `arn:aws:ssm:${this.region}:${this.account}:parameter${props.ssmTokensParam}`;
    const capabilityArn = `arn:aws:ssm:${this.region}:${this.account}:parameter${props.ssmCapabilitySecretParam}`;

    const fn = new NodejsFunction(this, 'Install', {
      entry: path.join(__dirname, '..', 'lambda', 'handler.ts'),
      runtime: lambda.Runtime.NODEJS_22_X,
      timeout: cdk.Duration.seconds(15),
      memorySize: 256,
      reservedConcurrentExecutions: 5,
      environment: {
        BUCKET: bucket.bucketName,
        SSM_TOKENS_PARAM: props.ssmTokensParam,
        SSM_CAPABILITY_SECRET_PARAM: props.ssmCapabilitySecretParam,
        FEEDBACK_ENDPOINT: props.feedbackEndpoint || '',
        PRESIGN_TTL: '300',
        CAPABILITY_TTL_DAYS: '90',
      },
      bundling: { minify: true, target: 'node22' },
    });

    // Least privilege: read the one token SecureString, read objects to presign and to serve install.sh.
    fn.addToRolePolicy(new iam.PolicyStatement({
      actions: ['ssm:GetParameter'],
      resources: [tokensArn, capabilityArn],
    }));
    bucket.grantRead(fn);

    const url = fn.addFunctionUrl({
      authType: lambda.FunctionUrlAuthType.NONE, // public: application capability auth is enforced
    });

    new cdk.CfnOutput(this, 'InstallUrl', {
      value: url.url,
      description: 'curl -fsSL <this>install.sh | CTX_TOKEN=... sh',
    });
    new cdk.CfnOutput(this, 'BucketName', { value: bucket.bucketName });
  }
}
