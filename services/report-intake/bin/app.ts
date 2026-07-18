import * as cdk from 'aws-cdk-lib';
import { ReportIntakeStack } from '../lib/stack';

const app = new cdk.App();
const githubRepo = String(app.node.tryGetContext('githubRepo') || '');
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(githubRepo)) {
  throw new Error('Pass -c githubRepo=<owner/private-feedback-repo>; no public default is allowed.');
}

new ReportIntakeStack(app, 'CtxReportIntake', {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT,
    region: process.env.CDK_DEFAULT_REGION,
  },
  // The repo issues are filed against, and the SSM SecureString holding the fine-grained PAT.
  githubRepo,
  ssmTokenParam: app.node.tryGetContext('ssmTokenParam') || '/ctx/report-intake/github-token',
  ssmTokensParam: app.node.tryGetContext('ssmTokensParam') || '/ctx/dist/alpha-tokens',
  ssmCapabilitySecretParam: app.node.tryGetContext('ssmCapabilitySecretParam') || '/ctx/beta/capability-secret',
});
