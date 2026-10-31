# Security Policy

Buku Token handles sensitive things: your Claude login is read from Keychain, and it stores a Cursor session cookie and an OpenRouter API key if you connect them. Please report problems privately.

## Reporting a vulnerability

Use [GitHub's private vulnerability reporting](https://github.com/fizipan/buku-token/security/advisories/new). Please don't open a public issue for security problems.

Include what you found, how to reproduce it, and which version or commit you tested. Never include real credentials.

This is a side project maintained in spare time, so I can't promise a response time, but I will take reports seriously.

## What counts

- A credential reaching a log, error message, file or IPC response, or being sent anywhere other than the service it belongs to.
- Private data reaching the share card, such as a project name, path or email.
- A write into another tool's files or folders.
- A network request that the README's Privacy section doesn't list.

## Supported versions

Only the latest commit on `main` is supported.
