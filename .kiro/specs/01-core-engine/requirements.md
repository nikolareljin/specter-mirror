# Portable core engine requirements

1. Given RGB24/RGBA/YUYV input with declared dimensions and byte stride, reject invalid dimensions, arithmetic overflow and insufficient buffers before output mutation.

2. For valid frames, produce deterministic grayscale, Sobel and Bayer output using caller-owned buffers with no OS dependencies or per-frame allocation.

3. Pass known-pattern, padded-stride, tiny-frame and malformed-input tests on the host; compile for i686 and wasm32.
