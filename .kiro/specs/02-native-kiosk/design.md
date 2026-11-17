# Native Linux kiosk design

Compare pinned v4l and nokhwa backends before selection. Keep capture outside the portable engine. Convert negotiated formats explicitly; use minifb X11 software buffers only after runtime verification. Avoid unbounded frame queues.
