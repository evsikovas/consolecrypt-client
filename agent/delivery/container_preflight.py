"""Offline image-reference preflight. Does not pull images or verify a signature."""

import argparse
import json
import re


def validate_image_reference(reference):
    if not isinstance(reference, str) or len(reference) > 320 or reference.count("@") != 1:
        raise ValueError("an immutable image@sha256:digest reference is required")
    name, suffix = reference.split("@")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", suffix):
        raise ValueError("exact lowercase SHA-256 image digest required")
    components = name.split("/")
    if not components or any(not component for component in components):
        raise ValueError("image name required")
    if len(components) > 1:
        registry = components.pop(0)
        match = re.fullmatch(r"[a-z0-9]+(?:[.-][a-z0-9]+)*(?::([1-9][0-9]{0,4}))?", registry)
        if not match or match.group(1) and int(match.group(1)) > 65535:
            raise ValueError("unsupported registry name or port")
    if any(not re.fullmatch(r"[a-z0-9]+(?:[._-][a-z0-9]+)*", component) for component in components):
        raise ValueError("untagged lowercase repository name required")
    return {"image": reference, "digest_pinned": True, "signature_verified": False,
            "required_next_step": "verify image signature and supply provenance using the approved release process"}


def main():
    parser = argparse.ArgumentParser(description="Offline digest-reference check only; no Docker calls")
    parser.add_argument("--image", required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(validate_image_reference(args.image), indent=2))
    except ValueError as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
