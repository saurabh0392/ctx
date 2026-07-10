import * as cdk from 'aws-cdk-lib';
import { Construct } from 'constructs';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { NodejsFunction } from 'aws-cdk-lib/aws-lambda-nodejs';
import * as path from 'path';

export interface CtxDistStackProps extends cdk.StackProps {
  // SSM SecureString holding the alpha token allowlist (one token per line, optional "= label").
  ssmTokensParam: string;
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

    const fn = new NodejsFunction(this, 'Install', {
      entry: path.join(__dirname, '..', 'lambda', 'handler.ts'),
      runtime: lambda.Runtime.NODEJS_20_X,
      timeout: cdk.Duration.seconds(15),
      memorySize: 256,
      environment: {
        BUCKET: bucket.bucketName,
        SSM_TOKENS_PARAM: props.ssmTokensParam,
        PRESIGN_TTL: '300',
      },
      bundling: { minify: true, target: 'node20' },
    });

    // Least privilege: read the one token SecureString, read objects to presign and to serve install.sh.
    fn.addToRolePolicy(new iam.PolicyStatement({
      actions: ['ssm:GetParameter'],
      resources: [tokensArn],
    }));
    bucket.grantRead(fn);

    const url = fn.addFunctionUrl({
      authType: lambda.FunctionUrlAuthType.NONE, // public: users have no AWS credentials
      cors: {
        allowedOrigins: ['*'],
        allowedMethods: [lambda.HttpMethod.GET, lambda.HttpMethod.POST],
        allowedHeaders: ['content-type'],
      },
    });

    new cdk.CfnOutput(this, 'InstallUrl', {
      value: url.url,
      description: 'curl -fsSL <this>install.sh | CTX_TOKEN=... sh',
    });
    new cdk.CfnOutput(this, 'BucketName', { value: bucket.bucketName });
  }
}
