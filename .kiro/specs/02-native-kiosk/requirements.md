# Native Linux kiosk requirements

1. On a verified i686 antiX target, capture a supported V4L2 format and display CPU-rendered frames through X11 without OpenGL or Vulkan.

2. If no camera is usable, select sample mode; handle unplug, bounded newest-frame capture and clean Escape shutdown.

3. Provide fullscreen operation, configuration, actual frame timing and a reproducible linked executable.
