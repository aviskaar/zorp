name: Feature Request
description: Suggest a feature or enhancement.
title: "[feature]: "
labels: ["enhancement"]
body:
  - type: markdown
    attributes:
      value: |
        Thanks for taking the time to suggest a feature!
  - type: textarea
    id: problem
    attributes:
      label: Is your feature request related to a problem?
      description: Describe the problem you are trying to solve.
      placeholder: I am always frustrated when...
    validations:
      required: true
  - type: textarea
    id: solution
    attributes:
      label: Describe the solution you would like.
      description: A clear and concise description of what you want to happen.
      placeholder: Tell us how the feature should work.
    validations:
      required: true
  - type: textarea
    id: alternatives
    attributes:
      label: Describe alternatives you have considered.
      description: Any alternative solutions or features you have thought about.
  - type: textarea
    id: context
    attributes:
      label: Additional context
      description: Add any other context or screenshots about the feature request here.
  - type: markdown
    attributes:
      value: |
        If your issue is a question or you want to discuss ideas, open a [Discussion](https://github.com/aviskaar/zorp/discussions) instead.
