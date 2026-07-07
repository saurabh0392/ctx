import * as cdk from 'aws-cdk-lib';
import { ReportIntakeStack } from '../lib/stack';

const app = new cdk.App();

new ReportIntakeStack(app, 'CtxReportIntake', {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT,
    region: process.env.CDK_DEFAULT_REGION,
  },
  // The repo issues are filed against, and the SSM SecureString holding the fine-grained PAT.
  githubRepo: app.node.tryGetContext('githubRepo') || 'saurabh0392/ctx',
  ssmTokenParam: app.node.tryGetContext('ssmTokenParam') || '/ctx/report-intake/github-token',
});
