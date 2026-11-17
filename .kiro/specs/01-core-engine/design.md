# Portable core engine design

Build on RgbFrame and FrameError. Add grayscale and Sobel as small modules before Bayer, palette and temporal stages. Specify grayscale rounding, Sobel borders and saturation in tests. Keep preprocessing and overlays independently composable.
