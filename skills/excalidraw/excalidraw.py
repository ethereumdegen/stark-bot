#!/usr/bin/env python3
"""Excalidraw validation and export utility.

Usage:
    python3 excalidraw.py validate '{"file": "path.excalidraw"}'
    python3 excalidraw.py export  '{"file": "path.excalidraw", "format": "png", "save_public": true}'
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile


def validate(data: dict) -> dict:
    """Validate an excalidraw JSON file for common issues."""
    file_path = data.get("file", "")
    if not file_path or not os.path.isfile(file_path):
        return {"valid": False, "errors": [f"File not found: {file_path}"], "element_count": 0}

    try:
        with open(file_path, "r") as f:
            doc = json.load(f)
    except json.JSONDecodeError as e:
        return {"valid": False, "errors": [f"Invalid JSON: {e}"], "element_count": 0}

    elements = doc.get("elements", [])
    errors = []

    # Check for duplicate IDs
    ids = [el.get("id") for el in elements if el.get("id")]
    seen = set()
    for eid in ids:
        if eid in seen:
            errors.append(f"Duplicate ID: {eid}")
        seen.add(eid)

    # Build lookup maps
    by_id = {el["id"]: el for el in elements if "id" in el}

    for el in elements:
        eid = el.get("id", "?")
        el_type = el.get("type", "")

        # No diamond shapes
        if el_type == "diamond":
            errors.append(f"Diamond shape found: {eid} — use styled rectangles instead")

        # boundElements <-> containerId consistency
        bound_elements = el.get("boundElements") or []
        for ref in bound_elements:
            ref_id = ref.get("id")
            if ref_id and ref_id in by_id:
                target = by_id[ref_id]
                if ref.get("type") == "text" and target.get("containerId") != eid:
                    errors.append(
                        f"boundElements mismatch: {eid} references text {ref_id}, "
                        f"but {ref_id}.containerId = {target.get('containerId')}"
                    )

        # containerId back-reference check
        container_id = el.get("containerId")
        if container_id and container_id in by_id:
            container = by_id[container_id]
            container_bound = container.get("boundElements") or []
            refs = [r.get("id") for r in container_bound]
            if eid not in refs:
                errors.append(
                    f"containerId mismatch: {eid}.containerId = {container_id}, "
                    f"but {container_id}.boundElements does not reference {eid}"
                )

        # Multi-point arrows need elbowed: true, roundness: null
        if el_type == "arrow":
            points = el.get("points", [])
            if len(points) > 2:
                if not el.get("elbowed"):
                    errors.append(f"Multi-point arrow {eid} missing elbowed: true")
                if el.get("roundness") is not None:
                    errors.append(f"Multi-point arrow {eid} should have roundness: null")

            # Arrow bounding box vs points check
            if points:
                xs = [p[0] for p in points]
                ys = [p[1] for p in points]
                expected_w = max(xs) - min(xs)
                expected_h = max(ys) - min(ys)
                actual_w = el.get("width", 0)
                actual_h = el.get("height", 0)
                if abs(actual_w - expected_w) > 2 or abs(actual_h - expected_h) > 2:
                    errors.append(
                        f"Arrow {eid} bounding box mismatch: "
                        f"expected ~{expected_w:.0f}x{expected_h:.0f}, "
                        f"got {actual_w}x{actual_h}"
                    )

    return {
        "valid": len(errors) == 0,
        "errors": errors,
        "element_count": len(elements),
    }


def export(data: dict) -> dict:
    """Export an excalidraw file to PNG or SVG using @excalidraw/utils via Node.js."""
    file_path = data.get("file", "")
    fmt = data.get("format", "png").lower()
    save_public = data.get("save_public", False)

    if fmt not in ("png", "svg"):
        return {"success": False, "error": f"Unsupported format: {fmt}"}

    if not file_path or not os.path.isfile(file_path):
        return {"success": False, "error": f"File not found: {file_path}"}

    # Determine output path
    basename = os.path.splitext(os.path.basename(file_path))[0]
    out_name = f"{basename}.{fmt}"

    if save_public:
        # Resolve public dir — check STARK_PUBLIC_DIR env or default to stark-backend/public
        public_dir = os.environ.get("STARK_PUBLIC_DIR", "")
        if not public_dir:
            # Try to find stark-backend/public relative to this script or CWD
            candidates = [
                os.path.join(os.getcwd(), "public"),
                os.path.join(os.path.dirname(__file__), "..", "..", "stark-backend", "public"),
            ]
            for c in candidates:
                real = os.path.realpath(c)
                if os.path.isdir(real):
                    public_dir = real
                    break
            if not public_dir:
                public_dir = candidates[0]
        os.makedirs(public_dir, exist_ok=True)
        output_path = os.path.join(public_dir, out_name)
    else:
        output_path = os.path.join(os.path.dirname(file_path) or ".", out_name)

    # Build a small Node.js script to do the export
    node_script = f"""
const fs = require('fs');
const {{ exportToSvg, exportToBlob }} = require('@excalidraw/utils');

(async () => {{
  const data = JSON.parse(fs.readFileSync({json.dumps(os.path.realpath(file_path))}, 'utf-8'));
  const elements = data.elements || [];
  const appState = data.appState || {{}};
  const files = data.files || {{}};

  if ({json.dumps(fmt)} === 'svg') {{
    const svg = await exportToSvg({{ elements, appState, files }});
    fs.writeFileSync({json.dumps(os.path.realpath(output_path))}, svg.outerHTML || svg.toString());
  }} else {{
    const blob = await exportToBlob({{ elements, appState, files, mimeType: 'image/png' }});
    const buf = Buffer.from(await blob.arrayBuffer());
    fs.writeFileSync({json.dumps(os.path.realpath(output_path))}, buf);
  }}
  console.log('OK');
}})();
"""

    # Write temp script and run
    with tempfile.NamedTemporaryFile(mode="w", suffix=".mjs", delete=False) as tmp:
        tmp.write(node_script)
        tmp_path = tmp.name

    try:
        result = subprocess.run(
            ["node", tmp_path],
            capture_output=True,
            text=True,
            timeout=30,
        )
        if result.returncode != 0:
            stderr = result.stderr.strip()
            # Common issue: @excalidraw/utils not installed
            if "Cannot find module" in stderr:
                return {
                    "success": False,
                    "error": "Node.js module @excalidraw/utils not found. Install with: npm install @excalidraw/utils",
                    "details": stderr,
                }
            return {"success": False, "error": stderr or "Export failed"}
    except FileNotFoundError:
        return {"success": False, "error": "Node.js not found in PATH"}
    except subprocess.TimeoutExpired:
        return {"success": False, "error": "Export timed out after 30 seconds"}
    finally:
        os.unlink(tmp_path)

    response = {
        "success": True,
        "output": output_path,
        "format": fmt,
    }
    if save_public:
        response["public_url"] = f"/public/{out_name}"
    return response


def main():
    if len(sys.argv) < 3:
        print(json.dumps({"error": "Usage: excalidraw.py <validate|export> '<json_args>'"}))
        sys.exit(1)

    action = sys.argv[1]
    try:
        args = json.loads(sys.argv[2])
    except json.JSONDecodeError as e:
        print(json.dumps({"error": f"Invalid JSON arguments: {e}"}))
        sys.exit(1)

    if action == "validate":
        result = validate(args)
    elif action == "export":
        result = export(args)
    else:
        result = {"error": f"Unknown action: {action}. Use 'validate' or 'export'."}

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
