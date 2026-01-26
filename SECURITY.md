# Security Policy

## Reporting Security Vulnerabilities

**DO NOT create a public GitHub issue to report a security vulnerability.**

### How to Report

Use GitHub's private vulnerability reporting feature:

1. Go to [Security Advisories](https://github.com/tos-network/tos/security/advisories/new)
2. Click "Report a vulnerability"
3. Provide:
   - A clear title describing the vulnerability
   - Detailed description of the issue
   - Steps to reproduce
   - Proof-of-concept exploit (required for bounty consideration)
   - Potential impact assessment
   - Any suggested mitigations

If you cannot use GitHub Security Advisories, contact the team via the channels listed in the repository. Do not include exploit details in public channels.

### What to Expect

- **Initial response**: Within 72 hours
- **Status updates**: At least weekly until resolution
- **Disclosure coordination**: We will work with you on responsible disclosure timing

Please enable two-factor authentication on your GitHub account before submitting.

## Scope

### In Scope

The following components are eligible for security review:

| Component | Description |
|-----------|-------------|
| **tos_daemon** | Node software, consensus, P2P networking, RPC |
| **tos_miner** | Mining software, stratum interface |
| **tos_wallet** | Wallet functionality, key management |
| **tos_common** | Shared libraries, cryptographic primitives |

### Vulnerability Categories

| Category | Examples |
|----------|----------|
| **Critical: Loss of Funds** | Theft without user signature, unauthorized transfers |
| **Critical: Consensus Violations** | Safety violations, invalid state acceptance |
| **Critical: Liveness** | Network halts requiring manual intervention |
| **High: DoS** | Remote resource exhaustion, node crashes |
| **Medium: RPC** | RPC-specific crashes, information disclosure |
| **Low: Local** | Issues requiring local access or user interaction |

### Out of Scope

The following are **not eligible** for security bounties:

- Bugs in third-party dependencies (report upstream)
- Social engineering attacks
- Physical attacks requiring device access
- Issues in test/example code not used in production
- Automated scanner output without developed proof-of-concept
- Denial of service via rate limiting (expected behavior)
- Issues requiring root/admin access to the host system
- Vulnerabilities in infrastructure not part of this repository

## Security Bounties

Security vulnerabilities may be eligible for token-based rewards at the sole discretion of the core team.

### Eligibility Requirements

- Submission **must** include a working proof-of-concept
- Issue must be in-scope as defined above
- Reporter must follow the responsible disclosure process
- First reporter of a unique vulnerability has priority
- Partial credit may be given for duplicate reports

### Bounty Terms

- Rewards are **discretionary** and not guaranteed
- Rewards are denominated in **tokens**, not cash
- All rewards are subject to **mainnet launch** and **vesting conditions**
- Speculative reports without proof-of-concept will be closed
- The core team determines severity and reward amounts

### Duplicate Reports

If multiple reporters submit the same vulnerability, rewards will be split with priority given to the first submission. Subsequent reports of the same issue receive proportionally smaller shares.

## Incident Response Process

### 1. Triage

Upon receiving a report, the security team will:
- Acknowledge receipt within 72 hours
- Assess severity and validity
- Determine affected components and versions

### 2. Investigation

- Reproduce the vulnerability
- Assess potential impact on mainnet/testnet
- Identify root cause

### 3. Remediation

- Develop and test fixes
- Code review by multiple team members
- Coordinate with reporter on fix verification

### 4. Disclosure

- Notify affected parties (validators, node operators) if critical
- Deploy fixes to affected networks
- Publish security advisory after fix is deployed
- Credit reporter (unless anonymity requested)

## Responsible Disclosure Guidelines

We ask security researchers to:

- **Do not** exploit vulnerabilities on live networks beyond proof-of-concept
- **Do not** access or modify other users' data
- **Do not** perform denial of service attacks on production systems
- **Do not** publicly disclose until fix is deployed and disclosure is coordinated
- **Do** provide sufficient detail to reproduce and verify the issue
- **Do** give reasonable time for remediation before disclosure

## Contact

For security matters only:
- GitHub Security Advisories (preferred)
- Repository maintainers via GitHub

For general questions, use GitHub Issues or Discussions.

---

*This policy may be updated at any time. Last updated: 2025-01*
