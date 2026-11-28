# Native Linux kiosk requirements

1. On a verified i686 antiX target, capture a supported V4L2 format and display CPU-rendered frames through X11 without OpenGL or Vulkan.

2. If no camera is usable, the default camera path shall select sample mode;
   explicit idle artwork mode shall return to artwork. Handle unplug, bounded
   newest-frame capture and clean Escape shutdown in both paths.

3. Provide fullscreen operation, configuration, actual frame timing and a reproducible linked executable.
