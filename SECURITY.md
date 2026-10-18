# Security policy

## Supported versions

Boxdeck is in early development. Only the latest release receives security fixes.

## Reporting a vulnerability

Please **don't** open a public issue for security problems.

Report them privately through GitHub's [private vulnerability reporting](https://github.com/oops-apps/boxdeck/security/advisories/new) for this repository. Include:

- what the issue is and what an attacker could do with it
- steps to reproduce, or a proof of concept
- the Boxdeck version and your OS

You'll get an acknowledgement within a week. Once a fix is ready, we'll publish an advisory and credit you unless you'd rather stay anonymous.

## Areas of particular interest

- the local MCP HTTP endpoint (authentication, Host and Origin checks)
- anything that lets input reach the `vagrant` command line or a shell
- access to SSH keys, tokens or other secrets
