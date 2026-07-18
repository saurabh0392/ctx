import * as cdk from 'aws-cdk-lib';
import { CtxDistStack } from '../lib/stack';

const app = new cdk.App();
const feedbackEndpoint = String(app.node.tryGetContext('feedbackEndpoint') || '');
if (!feedbackEndpoint.startsWith('https://')) {
  throw new Error('Pass -c feedbackEndpoint=<https intake function URL>.');
}

new CtxDistStack(app, 'CtxDist', {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT,
    region: process.env.CDK_DEFAULT_REGION,
  },
  // SSM SecureString holding the beta invite roster (one token per line).
  ssmTokensParam: app.node.tryGetContext('ssmTokensParam') || '/ctx/dist/alpha-tokens',
  ssmCapabilitySecretParam: app.node.tryGetContext('ssmCapabilitySecretParam') || '/ctx/beta/capability-secret',
  feedbackEndpoint,
});
