# 0016. GPU access in context containers

- Status: Accepted
- Date: 2026-10-17
- Source: spike [M2.3](../spikes/M2.3-context-container.md); host with an NVIDIA
  RTX 5090 (driving the monitor) and an AMD iGPU

## Context

Mesa drivers in the base image cover AMD and Intel through `/dev/dri`. NVIDIA's
proprietary driver needs userspace libraries that match the host's kernel
driver, so they cannot be part of the image. Without them, programs fell back to
the iGPU or to software rendering.

## Decision

- **All GPUs are passed** to a context; programs choose (Firefox picked the GPU
  that drives the monitor by itself).
- **Mesa (AMD/Intel):** `--device /dev/dri`.
- **NVIDIA:** if a CDI spec for NVIDIA exists on the host (`nvidia-ctk cdi list`
  shows `nvidia.com/gpu=all`), koje adds `--device nvidia.com/gpu=all`. CDI injects
  the host's libraries and registers them in the container's library cache.
- **NVIDIA's GBM backend** is missing from the spec that `nvidia-ctk` generates;
  koje mounts the host's `gbm/nvidia-drm_gbm.so` (resolved) read-only into the
  image's GBM backend directory. Only that file – the host's `gbm/` directory
  also holds the host's Mesa backend, which does not match the image.
- koje does not generate CDI specs (needs root); `koje` tells the user to run
  `sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml` when an NVIDIA GPU
  is present but no spec is found, and after driver updates.

## Consequences

+ Hardware rendering on NVIDIA and AMD/Intel without per-context setup.
- Host paths differ by distribution (the spike: Ubuntu's
  `/usr/lib/x86_64-linux-gnu`, the image's `/usr/lib64/gbm`); koje must find the
  GBM backend on the host and know the image's backend directory (M3.3).
- A driver update on the host needs a regenerated CDI spec; running contexts
  keep the old libraries until restarted.
