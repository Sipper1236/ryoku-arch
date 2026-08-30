# Task 2: Fail-safe standalone media renderer

## Status

Complete. Commit: `b60b46da2`, `[ryoku] shell: render custom reload cover media`

## TDD evidence

### RED

Before creating either renderer type, ran:

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
```

Result: expected failure (`0 passed, 1 failed`):

```text
Type Reload.ReloadMedia unavailable
.../reload-cover/ReloadMedia.qml: No such file or directory
```

### GREEN

After implementing `ReloadMedia.qml` and `ReloadVideo.qml`, the same focused command passed:

```text
PASS ReloadMedia::test_animated_image_becomes_custom_media()
PASS ReloadMedia::test_default_uses_bundled_wordmark()
PASS ReloadMedia::test_failure_phase_forces_default()
PASS ReloadMedia::test_missing_image_reports_error_and_keeps_default()
PASS ReloadMedia::test_valid_image_becomes_custom_media()
Totals: 7 passed, 0 failed, 0 skipped, 0 blacklisted
```

The missing-file test intentionally emits the Qt `AnimatedImage` read-error warning while asserting the public fallback state.

## Verification

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
Totals: 7 passed, 0 failed, 0 skipped, 0 blacklisted

tests/reload-cover.sh
reload cover launcher: PASS
```

No formatter, qmllint invocation, build, or broad test suite was run.

## Files

- `tests/qml/tst_reloadmedia.qml`: focused Qt Quick Test for default, still image, animation, absent file fallback, and forced default.
- `ryoku/shell/quickshell/reload-cover/ReloadMedia.qml`: standalone descriptor renderer. Its synchronous bundled wordmark remains visible until a custom image or lazy video is ready, and returns on any error or forced-default phase.
- `ryoku/shell/quickshell/reload-cover/ReloadVideo.qml`: lazily constructed Qt Multimedia leaf with muted, infinite, aspect-fit playback; it stops when inactive and is destroyed by its parent loader off-surface.
- `ryoku/shell/quickshell/reload-cover/ReloadCover.qml`: passes the descriptor to the media leaf, preserves the existing opacity/scale behavior, forces the bundled wordmark on failure, and derives failure-label placement from the default logo height.
- `ryoku/shell/quickshell/reload-cover/shell.qml`: blocking `brand.json` adapter and descriptor plumbing to each cover output, without lifecycle changes.
- `ryoku/shell/CHANGELOG.md`: Unreleased Added entry.

## Self-review

- `ReloadMedia` imports only `QtQuick`; Qt Multimedia is isolated to the Loader-created `ReloadVideo` type.
- All `Loader.item` reads are guarded by `Loader.Ready`, including the loader callback.
- Default artwork has no asynchronous or cache override, preserving synchronous fallback recovery; custom artwork has bounded `sourceSize`, async decode, and disabled cache.
- Video is muted, loops infinitely, preserves aspect fit, and is unloaded while inactive or forced to default.
- `ReloadCover` retains its existing `closeAnim`/`openAnim` durations, phase transitions, masks, black backgrounds, mapped reporting, and IPC/handshake path.
- Only the requested files were committed; the pre-existing modified `AGENTS.md` was not staged or changed.

## Concerns

None for the requested scope. The missing-media path logs the expected Qt image decoder warning during its negative test; the renderer exposes `mediaError` and keeps the bundled fallback visible.

## Fix round 1: expected missing-media decoder warning

### Test file

`tests/qml/tst_reloadmedia.qml`

The missing-image test now registers two narrow `ignoreWarning()` regular
expressions immediately before creating the media item. Qt emits the same
expected decoder warning twice; each assertion matches only:

```text
QML AnimatedImage: Error Reading Animated Image File file:///missing/reload-cover.png
```

### Warning output

Before the assertion, the focused test emitted two instances of:

```text
QWARN ... ReloadMedia.qml:38:5: QML AnimatedImage: Error Reading Animated Image File file:///missing/reload-cover.png
```

After the assertion, the focused command output is warning-free:

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
PASS   : qmltestrunner::ReloadMedia::initTestCase()
PASS   : qmltestrunner::ReloadMedia::test_animated_image_becomes_custom_media()
PASS   : qmltestrunner::ReloadMedia::test_default_uses_bundled_wordmark()
PASS   : qmltestrunner::ReloadMedia::test_failure_phase_forces_default()
PASS   : qmltestrunner::ReloadMedia::test_missing_image_reports_error_and_keeps_default()
PASS   : qmltestrunner::ReloadMedia::test_valid_image_becomes_custom_media()
PASS   : qmltestrunner::ReloadMedia::cleanupTestCase()
Totals: 7 passed, 0 failed, 0 skipped, 0 blacklisted
```

### Self-review

- The regular expression constrains the expected warning to `AnimatedImage` and
  the single missing fixture path; unrelated warnings remain visible.
- Registering the warning twice verifies both known decoder emissions occur and
  leaves the test log clean.
- Production QML, `AGENTS.md`, and the reload lifecycle were unchanged.

## Fix round 2: preserve custom-media aspect ratio

### RED

Added `objectName: "customImage"` to the custom `AnimatedImage` and extended
`tests/qml/tst_reloadmedia.qml` to find it after the wordmark is ready and
compare its painted ratio with `928 / 160`.

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
FAIL! ReloadMedia::test_valid_image_becomes_custom_media()
Actual: 1.7777777777777777
Expected: 5.8
Totals: 6 passed, 1 failed
```

This reproduces the preview stretch from the previous two-dimension
`sourceSize` binding.

### GREEN

Changed only the custom image decode bound from the two-dimension `sourceSize`
value to `sourceSize.width`, preserving the image decoder's native aspect while
retaining a bounded decode width.

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
PASS   : qmltestrunner::ReloadMedia::test_valid_image_becomes_custom_media()
Totals: 7 passed, 0 failed, 0 skipped, 0 blacklisted
```

### Files and self-review

- `ryoku/shell/quickshell/reload-cover/ReloadMedia.qml`: exposes the custom
  image for the focused test and bounds decoding by width only.
- `tests/qml/tst_reloadmedia.qml`: verifies the shipped 928x160 wordmark's
  painted aspect ratio after custom media becomes ready.
- `fillMode: Image.PreserveAspectFit` is unchanged.
- The `0.02` ratio tolerance is below 0.4% and accommodates integer painted
  dimensions (`640 / 110`) without accepting the prior 16:9 distortion.
- No lifecycle code, `AGENTS.md`, comments, or unrelated files changed.

## Fix round 3: orientation-aware bounded custom-media decoding

### RED

Added `tests/qml/fixtures/tall.png`, a 640x12800 accepted PNG, and extended
`tests/qml/tst_reloadmedia.qml` to assert both wide and tall custom media keep
their painted aspect ratio while requesting exactly one bounded source
dimension.

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
FAIL! ReloadMedia::test_tall_image_preserves_aspect_with_bounded_decode()
Actual: 640
Expected: 0
Totals: 7 passed, 1 failed
```

The old width-only renderer requested a 640-pixel decode width for the
640x12800 source rather than the required height-only 360-pixel viewport
bound.

### GREEN

`ReloadMedia` now asynchronously loads a 64-pixel-wide `AnimatedImage` probe
with cache disabled. Once its scaled implicit dimensions identify the
orientation, a loader constructs exactly one full media leaf:

- wide media sets only `sourceSize.width` to the viewport width;
- tall media sets only `sourceSize.height` to the viewport height.

The bundled wordmark remains visible during the probe and full-media load.

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
PASS   : qmltestrunner::ReloadMedia::test_tall_image_preserves_aspect_with_bounded_decode()
PASS   : qmltestrunner::ReloadMedia::test_valid_image_becomes_custom_media()
Totals: 8 passed, 0 failed, 0 skipped, 0 blacklisted
```

### Files and self-review

- `tests/qml/fixtures/tall.png`: 640x12800 raster fixture.
- `tests/qml/tst_reloadmedia.qml`: wide and tall aspect/decode-bound
  regression coverage.
- `ryoku/shell/quickshell/reload-cover/ReloadMedia.qml`: bounded probe and
  orientation-selected full-media leaf.
- Both `Loader.item` status reads require `Loader.Ready` and a non-null item.
- The missing-image assertion now expects one narrow `AnimatedImage` warning:
  the probe produces one decoder warning instead of the prior full-image
  renderer's two; the focused test output remains clean.
- The `Image` API specifies that a single positive `sourceSize` dimension
  preserves the other proportionally; this avoids the prior stretch while
  constraining tall-source memory.
- No lifecycle code, `AGENTS.md`, comments, formatters, builds, or broad suites
  changed or ran.

## Fix round 4: bounded, disposable orientation probe

### RED

Extended the wide and 640x12800 tall-media assertions to require a 64x64
orientation request, persisted orientation, and a null probe loader after
custom media becomes ready. Before the fix, the retained `AnimatedImage` probe
provided none of those properties:

```text
FAIL! ReloadMedia::test_tall_image_preserves_aspect_with_bounded_decode()
Uncaught exception: Cannot read property 'width' of undefined
FAIL! ReloadMedia::test_valid_image_becomes_custom_media()
Uncaught exception: Cannot read property 'width' of undefined
Totals: 6 passed, 2 failed, 0 skipped, 0 blacklisted
```

### GREEN

The probe is now a loader-owned `Image` constrained with
`sourceSize: Qt.size(64, 64)` and `Image.PreserveAspectFit`. Its ready handler
stores `imageTall`, then flips `orientationKnown`; that deactivates the loader
and destroys the probe before the full `AnimatedImage` leaf is created. The
full leaf continues to request only viewport width for wide media or viewport
height for tall media. The bundled wordmark remains visible until that full
leaf is ready.

```text
QT_QPA_PLATFORM=offscreen QSG_RHI_BACKEND=software /usr/lib/qt6/bin/qmltestrunner -input tests/qml -o -,txt
PASS   : qmltestrunner::ReloadMedia::test_animated_image_becomes_custom_media()
PASS   : qmltestrunner::ReloadMedia::test_default_uses_bundled_wordmark()
PASS   : qmltestrunner::ReloadMedia::test_failure_phase_forces_default()
PASS   : qmltestrunner::ReloadMedia::test_missing_image_reports_error_and_keeps_default()
PASS   : qmltestrunner::ReloadMedia::test_tall_image_preserves_aspect_with_bounded_decode()
PASS   : qmltestrunner::ReloadMedia::test_valid_image_becomes_custom_media()
Totals: 8 passed, 0 failed, 0 skipped, 0 blacklisted
```

### Self-review

- `Image.sourceSize` bounds both probe dimensions while
  `PreserveAspectFit` retains the true source orientation; the probe has no
  animation-frame cache.
- `orientationKnown` and `imageTall` outlive the probe. The test verifies that
  `imageProbeLoader.status` is `Loader.Null` and `item` is null after each
  wide/tall full-media load.
- The existing wide/tall assertions still verify painted aspect and the
  one-dimension full-media decode request. Video, forced-default behavior, and
  the fallback remain untouched.
- The negative test now expects the single bounded `Image` probe's narrow
  missing-file warning. No external process, dependency, descriptor, or
  fixture was added.

### Concerns

None for the requested scope. This uses Qt Quick's native bounded image decode
path; only the focused QML test command was run.
