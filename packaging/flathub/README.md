# Flathub Submission Files

`com.yanklog.YankLog.yml` is the Flathub-specific copy of the upstream Flatpak
manifest. It intentionally uses the immutable public Git tag and commit instead of
the upstream development manifest's local `dir` source.

When making the Flathub submission, copy this entire directory's two files into the
root of the separate `com.yanklog.YankLog` submission repository:

```text
com.yanklog.YankLog.yml
generated-sources.json
```

Before each submission or update, replace `tag` and `commit` together with the new
public release tag and its peeled commit. Regenerate and copy `generated-sources.json`
after every `Cargo.lock` change. Do not use a moving branch such as `master`.
