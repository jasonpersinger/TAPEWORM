#!/usr/bin/env python3
"""Fetch an official Docker Hub image (linux/amd64) into a plain root filesystem directory.

No Docker/Podman needed. Every layer blob is verified against its sha256 digest.

    oci-fetch.py library/debian bookworm            -> prints the manifest-list digest and the amd64 digest
    oci-fetch.py library/debian sha256:<digest> DIR -> extracts that exact amd64 image manifest into DIR

The second form is what the build uses: the image is pinned by digest, never by a mutable tag.
"""
import hashlib, io, json, os, shutil, sys, tarfile, urllib.request

REG = "https://registry-1.docker.io/v2"
ACCEPT = ", ".join([
    "application/vnd.oci.image.index.v1+json",
    "application/vnd.docker.distribution.manifest.list.v2+json",
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
])


def token(repo):
    url = f"https://auth.docker.io/token?service=registry.docker.io&scope=repository:{repo}:pull"
    return json.load(urllib.request.urlopen(url))["token"]


def get(repo, path, tok, accept=ACCEPT):
    req = urllib.request.Request(f"{REG}/{repo}/{path}",
                                 headers={"Authorization": f"Bearer {tok}", "Accept": accept})
    r = urllib.request.urlopen(req)
    return r.read(), r.headers


def amd64_manifest(repo, ref, tok):
    body, hdr = get(repo, f"manifests/{ref}", tok)
    top_digest = hdr.get("Docker-Content-Digest") or "sha256:" + hashlib.sha256(body).hexdigest()
    doc = json.loads(body)
    if "manifests" in doc:  # index / manifest list
        m = [x for x in doc["manifests"]
             if x["platform"]["architecture"] == "amd64" and x["platform"]["os"] == "linux"][0]
        body, _ = get(repo, f"manifests/{m['digest']}", tok)
        if "sha256:" + hashlib.sha256(body).hexdigest() != m["digest"]:
            sys.exit("amd64 manifest digest mismatch")
        return top_digest, m["digest"], json.loads(body)
    return top_digest, top_digest, doc


def main():
    repo, ref = sys.argv[1], sys.argv[2]
    tok = token(repo)
    top, amd, man = amd64_manifest(repo, ref, tok)
    print(f"index_digest={top}\namd64_digest={amd}")
    if len(sys.argv) < 4:
        return
    if ref.startswith("sha256:") and ref != amd:
        sys.exit(f"refusing: {ref} is not a single amd64 manifest (resolved {amd})")
    dest = sys.argv[3]
    os.makedirs(dest, exist_ok=False)
    for layer in man["layers"]:
        blob, _ = get(repo, f"blobs/{layer['digest']}", tok, accept="*/*")
        if "sha256:" + hashlib.sha256(blob).hexdigest() != layer["digest"]:
            sys.exit(f"layer digest mismatch {layer['digest']}")
        print(f"layer {layer['digest']} ok ({len(blob)} bytes)")
        with tarfile.open(fileobj=io.BytesIO(blob), mode="r:*") as t:
            for m in t.getmembers():
                name = os.path.basename(m.name)
                if name.startswith(".wh."):  # whiteout: delete from lower layer
                    victim = os.path.realpath(os.path.join(dest, os.path.dirname(m.name), name[4:]))
                    if not victim.startswith(os.path.realpath(dest) + os.sep):
                        sys.exit(f"whiteout escapes rootfs: {m.name}")
                    if os.path.isdir(victim) and not os.path.islink(victim):
                        shutil.rmtree(victim)
                    elif os.path.lexists(victim):
                        os.unlink(victim)
                    continue
                if m.isdev():  # cannot mknod rootless; bind /dev at run time instead
                    continue
                t.extract(m, dest, set_attrs=False, filter="tar")
                p = os.path.join(dest, m.name)
                if not m.issym():
                    os.chmod(p, m.mode | 0o200)  # keep owner-writable so apt/dpkg can work rootless
    with open(os.path.join(dest, ".oci-image"), "w") as f:
        f.write(f"repo={repo}\nindex_digest={top}\namd64_digest={amd}\n")


if __name__ == "__main__":
    main()
