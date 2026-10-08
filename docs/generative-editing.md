# Generative editing — coming soon

Open **Window › Generative AI…**, or the viewport task bar's **…** menu. Its four workflows are:

| Workflow | Local draft controls | Future generated result |
|---|---|---|
| Generate image | Prompt, width and height; works without an open document | Image on an empty canvas or a new layer |
| Extend image | Optional scene prompt and a larger canvas | Generated surroundings beyond the original edges |
| Generative reframe | Optional prompt, size and 1:1 / 4:5 / 16:9 / 9:16 presets | Expanded scene around the original image in the chosen frame |
| Edit selected object | Prompt and the current selection; canvas size stays fixed | Object replacement or modification within the selected region |

With a pixel or placed-image layer and a selection, **Edit object · Coming soon** appears on the
draggable viewport task bar and opens the floating prompt form. Select an object with Object
Selection, Select Subject, a marquee or a lasso first. These selection tools keep their current
local Classical / optional-model behavior. Remove Background remains fully functional.

The prompt form can be dragged by its title bar and scrolls when the viewport is small. Close
the form to return to the contextual task bar; the bar is hidden while the form is open so it
cannot cover the prompt controls.

**Review draft** validates the prompt and proposed geometry locally. It does not create a layer,
resize the canvas, edit pixels or masks, consume history, sample an image, upload a prompt, or
send a generation request. Changing the document invalidates the review. Prompts accept up to
2,000 Unicode characters; drafts are capped at 32,768 pixels per side and 100 million pixels.
Extension and reframing keep the original centred inside the proposed canvas. Reframing presets
expand the canvas to contain the original; they do not crop or stretch it. Selected-object
drafts require a nonempty selection and the current canvas dimensions.

**Generate · Coming soon** is disabled in every account and build state. The current official
[Sign in with ChatGPT limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
exclude image generation. Signing in, granting plan permission, reviewing a draft or supplying
automation parameters cannot override this gate. No alternate paid provider or API-key fallback
is included. The account settings link opens the existing optional ChatGPT sign-in window.

## Engine and automation

- `generative.capabilities {}` reports `available:false`, `status:"comingSoon"` and the four operations.
- `generative.prepare {operation,prompt,width?,height?}` returns a validated local metadata draft,
  including current document ID/revision, output dimensions, centred canvas origin and selection
  bounds where applicable. It returns `imageSampled:false` and `requestSent:false`.
- `generative.run` is disabled and returns an explicit coming-soon error, including when called
  directly. It is also denied to untrusted automation and nested action playback.
- `window.generativeAI {operation?}` opens a form. `ui.set {"generative":{"open":true,
  "operation":"generate","prompt":"A blue bicycle","width":1024,"height":768}}` sets local
  draft state; malformed fields fail before changing UI state. `ui.inspect.generative` exposes
  those draft controls, without account credentials. Draft queries are excluded from action journals.

All product code is Rust and works without native authentication, including the web UI. There is
no mock image output or success message pretending a generation occurred.

## Enabling generation later

An OpenAI support announcement will not automatically enable these buttons. A follow-up must
verify the documented route's actual image-generation capability and permissions, then implement
and test its provider adapter. That work must capture a stable source/selection snapshot, convert
model inputs through CMS, support cancellation and structured errors, and apply validated output
as an editable layer in one undo step without losing the original. Document/revision changes
during the request must prevent applying output to a different target. Windows/Linux real-provider
checks and image quality evaluation remain required. The current draft objects are metadata,
not image requests or a complete provider integration.

The backend decision is still deferred in the upstream roadmap (#41); this contributor change
is draft UX and local validation, with no claim of generative behavior or Photoshop AI parity.
