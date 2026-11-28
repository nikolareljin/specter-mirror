# Asset provenance

| Files | Origin | License | Purpose |
| --- | --- | --- | --- |
| branding/mark.svg, logo-dark.svg, logo-light.svg | Original geometric SVG paths and system-font text | Apache-2.0 | Product identity |
| branding/hero.svg | Original SVG composition using the product mark and mirror paths | Apache-2.0 | README hero |
| illustrations/antique-mirror.svg | Original SVG paths and gradients | Apache-2.0 | Interface concept artwork |
| web/public/favicon.svg | Copy of the original mark | Apache-2.0 | Browser icon |
| models/seeta_fd_frontal_v1.0.bin | SeetaFace frontal face detection model, copied unchanged from the rustface 0.1.7 crate (SHA-256 c4619d066ed35e84d9a8e842860b0dff567aba0cbb139881075538761db3ff5d) | BSD-2-Clause, VIPL group, ICT, Chinese Academy of Sciences; notice in models/SEETAFACE-LICENSE | Face location in the development preview, optional web face locator and native artwork detector process |
| artwork/island.jpg | Arnold Bocklin, Island of the Dead, 1880, The Metropolitan Museum of Art, object 435683 | CC0-1.0, Met Open Access | Offline native idle artwork |
| artwork/monsters.jpg | Goya, The Sleep of Reason Produces Monsters, 1799, impression published 1908-12, The Met, object 334453 | CC0-1.0, Met Open Access | Offline native idle artwork |
| artwork/skull.jpg | Pieter Claesz, Still Life with a Skull and a Writing Quill, 1628, The Met, object 435904 | CC0-1.0, Met Open Access | Offline native idle artwork |
| licenses/jpeg-decoder-0.3.2-LICENSE-MIT, licenses/jpeg-decoder-0.3.2-LICENSE-APACHE | Unchanged license files from the pinned jpeg-decoder 0.3.2 package | MIT OR Apache-2.0 | Preserve decoder notices in native bundles |
| licenses/rustface-0.1.7-LICENSE | Unchanged license file from the pinned rustface 0.1.7 package | BSD-2-Clause | Preserve detector notices in native bundles |
| web/src/sample.ts | Original procedural Canvas geometry and visitor timeline | Apache-2.0 | Generated sample input, not a recording or real person's likeness |

Created on 2026-11-16. No camera output is included. The
face model (added 2026-11-24) is third-party data, not original artwork.
The illustration is not a screenshot or an implemented camera effect.
No external fonts, official third-party logos or audio are used.

Museum JPEGs were downloaded unchanged from the Met's Open Access API on
2026-11-28. [sources.json](artwork/sources.json) pins each file's checksum,
object URL, download URL, date and artist. The runtime converts them to green
and scales with contain, without changing the source files. The Met permits
reuse and remixing of its Open Access images under CC0:
https://www.metmuseum.org/hubs/open-access.
