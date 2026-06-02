from __future__ import annotations

import time
from dataclasses import dataclass
from pathlib import Path

from PIL import ImageGrab

from utils.logging import get_logger

LOGGER = get_logger("blinky.capture")


@dataclass
class Screenshot:
    path: Path
    width: int        # screenshot pixel width (after thumbnail scaling)
    height: int       # screenshot pixel height (after thumbnail scaling)
    screen_width: int   # actual capture width before scaling (≈ physical screen width)
    screen_height: int  # actual capture height before scaling (≈ physical screen height)


_CAMERA = None

def capture_screen() -> Screenshot:
    """Capture the primary display with dxcam, falling back to PIL ImageGrab."""
    global _CAMERA
    captures_dir = Path("tmp") / "captures"
    captures_dir.mkdir(parents=True, exist_ok=True)
    path = captures_dir / f"screen-{int(time.time() * 1000)}.jpg"

    from PIL import Image
    try:
        resample_filter = Image.Resampling.LANCZOS
    except AttributeError:
        resample_filter = Image.LANCZOS

    image = None
    try:
        import dxcam

        if _CAMERA is None:
            _CAMERA = dxcam.create(output_color="RGB")
        
        frame = _CAMERA.grab()
        if frame is None:
            time.sleep(0.01)
            frame = _CAMERA.grab()

        if frame is None:
            raise RuntimeError("dxcam returned no frame")

        image = Image.fromarray(frame)
        LOGGER.info("Captured screen with dxcam")
    except Exception as exc:
        LOGGER.warning("dxcam capture failed, using ImageGrab: %s", exc)
        image = ImageGrab.grab(all_screens=False)

    # Record the original capture resolution BEFORE any thumbnail scaling.
    # UIA element coordinates are screen-absolute in this space.
    screen_w, screen_h = image.width, image.height

    image.thumbnail((1920, 1080), resample=resample_filter)
    image = image.convert("RGB")
    image.save(path, format="JPEG", quality=75, optimize=True)
    LOGGER.info(
        "Saved optimized screenshot: %s (size: %dx%d, screen: %dx%d)",
        path, image.width, image.height, screen_w, screen_h,
    )

    return Screenshot(
        path=path,
        width=image.width,
        height=image.height,
        screen_width=screen_w,
        screen_height=screen_h,
    )
