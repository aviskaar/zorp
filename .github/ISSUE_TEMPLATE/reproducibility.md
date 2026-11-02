name: Reproducibility Problem
description: Report a non-deterministic or hard-to-reproduce issue.
title: "[reproducibility]: "
labels: ["bug", "reproducibility"]
body:
  - type: markdown
    attributes:
      value: |
        Use this template when a bug does not happen consistently, or when a result cannot be reproduced reliably.
  - type: textarea
    id: what-happened
    attributes:
      label: What happened?
      description: Describe the symptom. How often does it happen?
      placeholder: Describe the issue and its frequency.
    validations:
      required: true
  - type: textarea
    id: reproduction-steps
    attributes:
      label: Steps that may trigger the issue
      description: List the steps you take. Include any variables or randomness involved.
      placeholder: |
        1.
        2.
        3.
    validations:
      required: true
  - type: textarea
    id: expected-vs-actual
    attributes:
      label: Expected vs. actual behavior
      description: What did you expect to see versus what you actually see?
      placeholder: Expected: ...  Actual: ...
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
  - type: input
    id: model
    attributes:
      label: Model used
      description: If relevant, which model or provider?
      placeholder: "e.g. claude-3.5-sonnet"
  - type: textarea
    id: logs
    attributes:
      label: Relevant log output
      description: Please copy and paste any relevant log output. This will be automatically formatted into code.
      render: shell
  - type: textarea
    id: additional
    attributes:
      label: Additional context
      description: Anything else that might help diagnose this.
  - type: markdown
    attributes:
      value: |
        If your issue is a question or you want to discuss ideas, open a [Discussion](https://github.com/aviskaar/zorp/discussions) instead.
