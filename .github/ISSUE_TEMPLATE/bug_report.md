name: Bug Report
description: File a bug report.
title: "[bug]: "
labels: ["bug"]
body:
  - type: markdown
    attributes:
      value: |
        Thanks for taking the time to fill out this bug report.
  - type: textarea
    id: what-happened
    attributes:
      label: What happened?
      description: Describe the bug and what you expected to happen.
      placeholder: Tell us what went wrong.
    validations:
      required: true
  - type: textarea
    id: reproduction-steps
    attributes:
      label: Steps to reproduce
      description: List the steps so we can reproduce the bug.
      placeholder: |
        1.
        2.
        3.
    validations:
      required: true
  - type: dropdown
    id: os
    attributes:
      label: Operating system
      multiple: false
      options:
        - macOS
        - Linux
        - Windows
    validations:
      required: true
  - type: input
    id: rust-version
    attributes:
      label: Rust version
      description: Run `rustc --version` to find this.
      placeholder: "e.g. 1.82.0"
    validations:
      required: true
  - type: dropdown
    id: zorp-version
    attributes:
      label: zorp version
      description: Run `zorp --version` or check the commit sha.
      placeholder: "e.g. v0.8.0 or a4b2c3d"
    validations:
      required: true
  - type: textarea
    id: logs
    attributes:
      label: Relevant log output
      description: Please copy and paste any relevant log output. This will be automatically formatted into code.
      render: shell
  - type: markdown
    attributes:
      value: |
        If your issue is a question or you want to discuss ideas, open a [Discussion](https://github.com/aviskaar/zorp/discussions) instead.
