---
inclusion: always
---

# Architecture

Keep spook-core portable and free of OS/window/camera dependencies. It owns checked frame contracts and reusable processing buffers. spook-kiosk owns V4L2/X11; spook-web owns WASM boundaries. Keep capture queues bounded and return errors before output mutation.
