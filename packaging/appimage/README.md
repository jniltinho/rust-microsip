# AppImage (convenience artifact)

The ELF `rust-microsip` is the primary artifact. The AppImage wraps the same ELF
(byte-identical) with `AppRun`, a desktop entry, an icon and the license notices.
It bundles no shared libraries: like the ELF, it needs glibc and `libasound.so.2`
from the host. Passing the AppImage tests is not evidence for the ELF and vice versa.

Build (exports the ELF, the AppImage and `SHA256SUMS` covering both):

```sh
docker buildx build --platform linux/amd64 --target artifacts --output type=local,dest=dist .
```

Run without FUSE:

```sh
./rust-microsip-<version>-x86_64.AppImage --appimage-extract-and-run --version
# or extract once and run the extracted tree:
./rust-microsip-<version>-x86_64.AppImage --appimage-extract
squashfs-root/AppRun --version
```

Verify an AppImage against its ELF (no FUSE needed):

```sh
sh packaging/appimage/verify.sh dist/rust-microsip-<version>-x86_64.AppImage dist/rust-microsip
```

The runtime is the official AppImage type-2 runtime, pinned by release URL and
SHA-256 in `pins.txt`; its MIT license and the notices of the libraries it links
statically are in `runtime-licenses/` and are copied into
`usr/share/doc/rust-microsip/appimage-runtime-licenses/` inside the image.
