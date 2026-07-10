import * as cdk from 'aws-cdk-lib';
import { CtxDistStack } from '../lib/stack';

const app = new cdk.App();

new CtxDistStack(app, 'CtxDist', {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT,
    region: process.env.CDK_DEFAULT_REGION,
  },
  // SSM SecureString holding the alpha token allowlist (one token per line).
  ssmTokensParam: app.node.tryGetContext('ssmTokensParam') || '/ctx/dist/alpha-tokens',
});
