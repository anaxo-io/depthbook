# Security Policy

## Supported versions

The latest released version receives security fixes. This crate has not yet reached 1.0; the API may change between minor versions.

## Reporting a vulnerability

Report security issues privately to **security@anaxo.io**. Please do not open a public issue.

Include:

- a description of the issue and its impact,
- the version or commit affected,
- steps to reproduce, ideally a failing test.

You will receive an acknowledgement within 72 hours. Fixes are disclosed publicly once released, or after 90 days, whichever comes first.

## Scope

This crate holds order book state in process memory. It performs no I/O, no network access, and contains no `unsafe` code (`#![forbid(unsafe_code)]`). The most likely classes of issue are integer overflow in scale-9 arithmetic and incorrect book state after malformed input — both are in scope.
