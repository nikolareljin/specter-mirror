# Privacy

The browser demo requests the camera only after you press Start camera. It
asks for video only, with no audio. Frames are processed locally by WebAssembly
and drawn to a canvas; they are not uploaded, recorded or sent to analytics.
Stop, switching to the sample, leaving the page or losing the camera releases
all media tracks. The generated sample scene never requests permission.
The native application is offline. Snapshots, if introduced, require separate
explicit consent.

Hardware diagnostics can contain device identifiers. Keep raw output local,
review redacted facts and publish only what is needed to explain compatibility.
