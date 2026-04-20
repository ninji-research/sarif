# Contributing to NINJI

Thank you for your interest in contributing to NINJI's infrastructure and documentation. We are committed to maintaining the highest engineering standards and ensuring a clean legal foundation for all our projects.

## Developer Certificate of Origin (DCO)

We do **not** require you to sign a Contributor License Agreement (CLA). Instead, we use the Developer Certificate of Origin (DCO). The DCO is a lightweight way for contributors to certify that they wrote or otherwise have the right to submit the code they are contributing to the project.

By contributing to this repository, you agree to the terms of the DCO (printed below).

### How to Sign Your Work

To agree to the DCO, you simply need to add a `Signed-off-by` line to your commit messages. This is a standard practice in open-source development ().

```text
Signed-off-by: Random J Developer <random@developer.example.org>
```

Git makes this easy. You can add the sign-off automatically by using the `-s` or `--signoff` flag when you commit:

```bash
git commit -s -m "feat: implement quantum-resistant edge router"
```

If you have authored multiple commits, please ensure that *every* commit is signed off. Pull requests containing commits without a valid `Signed-off-by` line will be blocked until the sign-offs are added.

---

### The Developer Certificate of Origin (Version 1.1)

```text
By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

## Pull Request Process

1.  **Fork and Branch:** Fork the repository and create a descriptive branch name.
2.  **Ensure Code Quality:** Follow the exact architectural constraints, zero-JS styling logic, and typing standards present in the repository.
3.  **Sign Your Commits:** Use `git commit -s`.
4.  **Submit PR:** Open a pull request against the `main` branch detailing the intent and testing strategy for your change.

*Note: NINJI retains the right to reject contributions that do not align with our performance standards, architectural directives, or legal requirements.*
