#!/usr/bin/env python3
"""
KiCad PCB export script for PCB3D format
This script is embedded in the Rust binary and executed at runtime
"""

import sys
import json
import os
import re
import subprocess
import shutil
from pathlib import Path
import tempfile

# Read PYTHONPATH environment variable and add all folders to the search path
# This is needed to find pcbnew module from KiCad installation
python_path = os.environ.get("PYTHONPATH", "")
path_separator = os.pathsep  # Use OS-specific path separator (: on Unix/Mac, ; on Windows)
if python_path:
    for path in python_path.split(path_separator):
        if path and path not in sys.path:
            sys.path.append(path)

try:
    import pcbnew
except ImportError:
    print("Error: pcbnew module not found. Please ensure KiCad is installed and python3 can access it.", file=sys.stderr)
    print(f"Python search paths: {sys.path}", file=sys.stderr)
    print(f"PYTHONPATH env var: {python_path}", file=sys.stderr)
    sys.exit(1)

def sanitize_name(name):
    """Sanitize names for file system usage"""
    return re.sub(r'[\W]+', '_', name)

def to_mm(value):
    """Convert KiCad internal units to millimeters"""
    return pcbnew.ToMM(value)

def to_mm_2d(point):
    """Convert KiCad point to millimeters"""
    return (pcbnew.ToMM(point.x), pcbnew.ToMM(point.y))

def hex_to_rgb(hex_string):
    """Convert hex color to RGB tuple (0-1 range)"""
    return (
        int(hex_string[0:2], 16) / 255.0,
        int(hex_string[2:4], 16) / 255.0,
        int(hex_string[4:6], 16) / 255.0,
    )

def get_pad_type(pad):
    """Map KiCad pad attribute to PadType enum name"""
    attr = pad.GetAttribute()
    if attr == pcbnew.PAD_ATTRIB_PTH:
        return "Tht"
    elif attr == pcbnew.PAD_ATTRIB_SMD:
        return "Smd"
    elif attr == pcbnew.PAD_ATTRIB_CONN:
        return "Conn"
    elif attr == pcbnew.PAD_ATTRIB_NPTH:
        return "Npth"
    else:
        return "Unknown"

def get_pad_shape(pad):
    """Map KiCad pad shape to PadShape enum name"""
    shape = pad.GetShape()
    if shape == pcbnew.PAD_SHAPE_CIRCLE:
        return "Circle"
    elif shape == pcbnew.PAD_SHAPE_RECT:
        return "Rect"
    elif shape == pcbnew.PAD_SHAPE_OVAL:
        return "Oval"
    elif shape == pcbnew.PAD_SHAPE_TRAPEZOID:
        return "Trapezoid"
    elif shape == pcbnew.PAD_SHAPE_ROUNDRECT:
        return "RoundRect"
    elif shape == pcbnew.PAD_SHAPE_CHAMFERED_RECT:
        return "ChamferedRect"
    elif shape == pcbnew.PAD_SHAPE_CUSTOM:
        return "Custom"
    else:
        return "Unknown"

def get_drill_shape(pad):
    """Map KiCad drill shape to DrillShape enum name"""
    shape = pad.GetDrillShape()
    if shape == pcbnew.PAD_DRILL_SHAPE_CIRCLE:
        return "Circular"
    elif shape == pcbnew.PAD_DRILL_SHAPE_OBLONG:
        return "Oval"
    else:
        return "Unknown"

def get_pad_fab_type(pad):
    """Map KiCad pad property to PadFabType enum name"""
    prop = pad.GetProperty()
    if prop == pcbnew.PAD_PROP_NONE:
        return "None"
    elif prop == pcbnew.PAD_PROP_BGA:
        return "Bga"
    elif prop == pcbnew.PAD_PROP_FIDUCIAL_GLBL or prop == pcbnew.PAD_PROP_FIDUCIAL_LOCAL:
        return "Fiducial"
    elif prop == pcbnew.PAD_PROP_TESTPOINT:
        return "TestPoint"
    elif prop == pcbnew.PAD_PROP_HEATSINK:
        return "Heatsink"
    elif prop == pcbnew.PAD_PROP_CASTELLATED:
        return "Castellated"
    else:
        return "None"  # Default to None

def export_layers(board, bounds, output_dir):
    """Export PCB layers as SVG files"""
    os.makedirs(output_dir, exist_ok=True)
    
    plot_controller = pcbnew.PLOT_CONTROLLER(board)
    plot_options = plot_controller.GetPlotOptions()
    plot_options.SetOutputDirectory(output_dir)
    
    plot_options.SetPlotFrameRef(False)
    plot_options.SetAutoScale(False)
    plot_options.SetScale(1)
    plot_options.SetMirror(False)
    plot_options.SetUseGerberAttributes(True)
    plot_options.SetDrillMarksType(pcbnew.DRILL_MARKS_NO_DRILL_SHAPE)
    
    # Include these layers
    layers = [
        "F_Cu", "F_Paste", "F_SilkS", "F_Mask",
        "B_Cu", "B_Paste", "B_SilkS", "B_Mask"
    ]
    
    for layer_name in layers:
        layer_id = getattr(pcbnew, layer_name, None)
        if layer_id is None:
            continue
            
        plot_controller.SetLayer(layer_id)
        plot_controller.OpenPlotfile(layer_name, pcbnew.PLOT_FORMAT_SVG, "")
        plot_controller.PlotLayer()
        filepath = Path(plot_controller.GetPlotFileName())
        plot_controller.ClosePlot()
        
        # Rename to standard name
        new_path = output_dir / f"{layer_name}.svg"
        if filepath.exists():
            filepath.rename(new_path)
            
            # Update SVG viewport
            content = new_path.read_text(encoding='utf-8')
            width = f"{bounds['size'][0]:.6f}mm"
            height = f"{bounds['size'][1]:.6f}mm"
            viewBox = f"{bounds['top_left'][0]:.6f} {bounds['top_left'][1]:.6f} {bounds['size'][0]:.6f} {bounds['size'][1]:.6f}"
            
            # Replace the SVG header
            content = re.sub(
                r'<svg([^>]*)width="[^"]*"[^>]*height="[^"]*"[^>]*viewBox="[^"]*"[^>]*>',
                f'<svg\\1width="{width}" height="{height}" viewBox="{viewBox}">',
                content
            )
            new_path.write_text(content, encoding='utf-8')

def get_stackup(board):
    """Extract stackup information from the board"""
    stackup = {
        "thickness_mm": 1.6,  # Default
        "mask_color": "Green",
        "mask_color_custom": [0.0, 0.0, 0.0],
        "silks_color": "White", 
        "silks_color_custom": [0.0, 0.0, 0.0],
        "surface_finish": "Hasl"
    }
    
    # Try to get stackup from board file
    # Save board to temporary file to parse it
    with tempfile.NamedTemporaryFile(suffix='.kicad_pcb', mode='w', delete=False) as f:
        temp_path = f.name
    
    try:
        pcbnew.SaveBoard(temp_path, board, aSkipSettings=True)
        with open(temp_path, 'r') as f:
            content = f.read()
        
        # Parse stackup section
        stackup_match = re.search(r'\(stackup\s*(?:\s*\([^\(\)]*(?:\([^\)]*\)\s*)*\)\s*)*\)', content, re.MULTILINE)
        if stackup_match:
            stackup_content = stackup_match.group(0)
            
            # Extract thickness
            thickness_matches = re.finditer(r'\(thickness\s+([^) ]*)[^)]*\)', stackup_content)
            total_thickness = sum(float(m.group(1)) for m in thickness_matches)
            if total_thickness > 0:
                stackup["thickness_mm"] = total_thickness
            
            # Extract mask color
            mask_match = re.search(r'\(layer\s+"[FB].Mask"\s+(?:\([^()]*\)\s+)*?\(color\s+"([^\)]*)"', stackup_content, re.DOTALL)
            if mask_match:
                color = mask_match.group(1)
                if color.startswith('#'):
                    stackup["mask_color"] = "Custom"
                    stackup["mask_color_custom"] = list(hex_to_rgb(color[1:7]))
                else:
                    # Capitalize first letter only for enum compatibility
                    color_map = {
                        "GREEN": "Green", "RED": "Red", "BLUE": "Blue",
                        "PURPLE": "Purple", "BLACK": "Black", "WHITE": "White",
                        "YELLOW": "Yellow"
                    }
                    stackup["mask_color"] = color_map.get(color.upper(), "Green")
            
            # Extract silkscreen color
            silks_match = re.search(r'\(layer\s+"[FB].SilkS"\s+(?:\([^()]*\)\s+)*?\(color\s+"([^\)]*)"', stackup_content, re.DOTALL)
            if silks_match:
                color = silks_match.group(1)
                if color.startswith('#'):
                    stackup["silks_color"] = "Custom"
                    stackup["silks_color_custom"] = list(hex_to_rgb(color[1:7]))
                else:
                    # Capitalize first letter only for enum compatibility
                    color_map = {
                        "GREEN": "Green", "RED": "Red", "BLUE": "Blue",
                        "PURPLE": "Purple", "BLACK": "Black", "WHITE": "White",
                        "YELLOW": "Yellow"
                    }
                    stackup["silks_color"] = color_map.get(color.upper(), "White")
            
            # Extract surface finish
            finish_match = re.search(r'\(copper_finish\s+"([^"]*)"', stackup_content)
            if finish_match:
                finish_map = {
                    "ENIG": "Enig",
                    "ENEPIG": "Enig",
                    "Hard gold": "Enig",
                    "Immersion gold": "Enig",
                    "HT_OSP": "None",
                    "OSP": "None",
                    "None": "None",
                }
                stackup["surface_finish"] = finish_map.get(finish_match.group(1), "Hasl")
    finally:
        if os.path.exists(temp_path):
            os.unlink(temp_path)
    
    return stackup

def get_board_definitions(board):
    """Extract board definitions from Edge.Cuts layer and stacking annotations"""
    boards = {}
    stacking_info = []
    
    # First, find all board outlines from Edge.Cuts layer
    # Group Edge.Cuts segments by connectivity to identify separate boards
    edge_segments = []
    for drawing in board.GetDrawings():
        if drawing.GetLayerName() == "Edge.Cuts":
            edge_segments.append(drawing)
    
    # For now, treat the entire Edge.Cuts as one main board
    # In the future, we could detect separate closed polygons for multi-board panels
    if edge_segments:
        # Calculate bounding box of all edge cuts
        min_x = min_y = float('inf')
        max_x = max_y = float('-inf')
        
        for seg in edge_segments:
            if hasattr(seg, 'GetStart'):
                start = to_mm_2d(seg.GetStart())
                end = to_mm_2d(seg.GetEnd())
                min_x = min(min_x, start[0], end[0])
                min_y = min(min_y, start[1], end[1])
                max_x = max(max_x, start[0], end[0])
                max_y = max(max_y, start[1], end[1])
            elif hasattr(seg, 'GetPosition'):
                # Handle other shape types (circles, arcs, etc.)
                bbox = seg.GetBoundingBox()
                min_x = min(min_x, to_mm(bbox.GetLeft()))
                min_y = min(min_y, to_mm(bbox.GetTop()))
                max_x = max(max_x, to_mm(bbox.GetRight()))
                max_y = max(max_y, to_mm(bbox.GetBottom()))
        
        # Create main board entry
        boards["main"] = {
            "bounds": {
                "top_left": [min_x, min_y],
                "size": [max_x - min_x, max_y - min_y]
            },
            "stacked_boards": {}
        }
    
    # Look for stacking annotations (keep this for board stacking relationships)
    for drawing in board.GetDrawings():
        if drawing.Type() == pcbnew.PCB_TEXT_T:
            text = drawing.GetText()
            
            # Look for named board regions (optional - for multi-board panels)
            if text.startswith("PCB3D_BOARD_"):
                board_name = sanitize_name(text[12:])
                # Could implement region detection here if needed
                
            # Look for stacking relationships
            elif text.startswith("PCB3D_STACK_"):
                parts = text[12:].split("_")
                if len(parts) >= 4 and parts[1] == "ONTO":
                    other_name = sanitize_name(parts[0])
                    target_name = sanitize_name(parts[2])
                    try:
                        z_offset = float(parts[3])
                        pos = to_mm_2d(drawing.GetPosition())
                        stacking_info.append((other_name, target_name, z_offset, pos))
                    except (ValueError, IndexError):
                        pass
    
    # Apply stacking information
    for other_name, target_name, z_offset, pos in stacking_info:
        # Default to main board if target not found
        if target_name not in boards:
            target_name = "main"
        
        if target_name in boards:
            target_pos = boards[target_name]["bounds"]["top_left"]
            boards[target_name]["stacked_boards"][other_name] = {
                "offset": [
                    pos[0] - target_pos[0],
                    pos[1] - target_pos[1],
                    z_offset
                ]
            }
    
    return boards

def export_pcb3d(pcb_path, output_dir):
    """Main export function"""
    # Try to create a wx app context for pcbnew (needed for some operations)
    try:
        import wx
        app = wx.App(False)  # False means no GUI
    except:
        pass  # Continue without wx context
    
    # Load the board
    board = pcbnew.LoadBoard(pcb_path)
    
    if board is None:
        print(f"ERROR: Failed to load board from {pcb_path}", file=sys.stderr)
        # Try to create a basic fallback result
        return {
            "wrl_path": str(output_dir / "pcb.wrl"),
            "components_dir": str(output_dir / "components"),
            "layers_dir": str(output_dir / "layers"),
            "bounds": {"top_left": [0, 0], "size": [100, 100]},
            "stackup": {},
            "boards": {"main": {"bounds": {"top_left": [0, 0], "size": [100, 100]}, "stacked_boards": {}}},
            "pads": {}
        }
    
    print(f"Board loaded successfully", file=sys.stderr)
    
    # Calculate bounds with margin
    box = board.ComputeBoundingBox(aBoardEdgesOnly=True)
    margin_nm = pcbnew.FromMM(1.0)  # 1mm margin
    bounds = {
        "top_left": [to_mm(box.GetLeft() - margin_nm), to_mm(box.GetTop() - margin_nm)],
        "size": [to_mm(box.GetWidth() + 2 * margin_nm), to_mm(box.GetHeight() + 2 * margin_nm)]
    }
    
    # Export VRML model
    wrl_path = output_dir / "pcb.wrl"
    components_dir = output_dir / "components"
    components_dir.mkdir(exist_ok=True)
    
    # Export VRML using pcbnew API (same as the original pcb2blender_exporter)
    export_success = False
    
    # First try pcbnew API
    try:
        # Use the exact same parameters as the original pcb2blender_exporter/export.py
        # Parameters: path, scale, export3D, useRelativePaths, usePlainPCB, refPlain, modelDir, xRef, yRef
        result = pcbnew.ExportVRML(
            str(wrl_path),           # Output file path
            0.001,                   # Scale (mm to meters conversion like original)
            True,                    # Export 3D files
            False,                   # Use relative paths
            True,                    # Use plain PCB
            True,                    # Reference plain
            str(components_dir),     # Components directory
            0.0,                     # X reference
            0.0                      # Y reference
        )
        
        # Check if file was created
        if wrl_path.exists():
            file_size = wrl_path.stat().st_size
            if file_size > 1000:  # At least 1KB to be valid
                export_success = True
            
    except Exception as e:
        print(f"pcbnew.ExportVRML exception: {e}", file=sys.stderr)
        import traceback
        traceback.print_exc(file=sys.stderr)
        
    if not export_success:
        # Try alternative: use kicad-cli as fallback
        print(f"Trying kicad-cli as fallback", file=sys.stderr)
        kicad_cli = shutil.which("kicad-cli")
        if not kicad_cli:
            # Try common locations
            possible_paths = [
                "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli",
                "/usr/bin/kicad-cli",
                "/usr/local/bin/kicad-cli",
            ]
            for path in possible_paths:
                if os.path.exists(path):
                    kicad_cli = path
                    break
        
        if kicad_cli:
            print(f"Found kicad-cli at: {kicad_cli}", file=sys.stderr)
            cmd = [
                kicad_cli,
                "pcb",
                "export",
                "vrml",
                str(pcb_path),
                "-o",
                str(wrl_path),
                "--units", "mm"
            ]
            
            print(f"Running: {' '.join(cmd)}", file=sys.stderr)
            result = subprocess.run(cmd, capture_output=True, text=True)
            
            if result.returncode == 0 and wrl_path.exists():
                file_size = wrl_path.stat().st_size
                print(f"VRML created via kicad-cli fallback: {file_size} bytes", file=sys.stderr)
                export_success = True
            else:
                print(f"kicad-cli failed: {result.stderr}", file=sys.stderr)
        
    if not export_success:
        print(f"All export attempts failed, creating fallback board geometry", file=sys.stderr)
        # Create a VRML with actual board dimensions
        board_width = bounds["size"][0] / 1000.0  # Convert mm to meters
        board_height = bounds["size"][1] / 1000.0
        board_thickness = 0.0016  # Standard 1.6mm PCB thickness
        center_x = (bounds["top_left"][0] + bounds["size"][0]/2) / 1000.0
        center_y = (bounds["top_left"][1] + bounds["size"][1]/2) / 1000.0
        
        wrl_path.write_text(f"""#VRML V2.0 utf8
Transform {{
  translation {center_x} {center_y} {board_thickness/2}
  children [
    Shape {{
      appearance Appearance {{
        material Material {{
          diffuseColor 0.05 0.3 0.05
          ambientIntensity 0.3
          specularColor 0.1 0.1 0.1
          shininess 0.2
        }}
      }}
      geometry Box {{
        size {board_width} {board_height} {board_thickness}
      }}
    }}
  ]
}}""")
    
    # Export layers
    layers_dir = output_dir / "layers"
    export_layers(board, bounds, layers_dir)
    
    # Get stackup
    stackup = get_stackup(board)
    
    # Get board definitions
    board_defs = get_board_definitions(board)
    
    # Extract pad information
    pads = {}
    for footprint in board.Footprints():
        has_model = len(footprint.Models()) > 0
        is_tht_or_smd = bool(footprint.GetAttributes() & (pcbnew.FP_THROUGH_HOLE | pcbnew.FP_SMD))
        value = footprint.GetValue()
        reference = footprint.GetReference()
        
        for i, pad in enumerate(footprint.Pads()):
            name = sanitize_name(f"{value}_{reference}_{footprint.m_Uuid.AsString()}_{i}")
            is_flipped = pad.IsFlipped()
            has_paste = pad.IsOnLayer(pcbnew.B_Paste if is_flipped else pcbnew.F_Paste)
            
            pads[name] = {
                "position": list(to_mm_2d(pad.GetPosition())),
                "is_flipped": is_flipped,
                "has_model": has_model,
                "is_tht_or_smd": is_tht_or_smd,
                "has_paste": has_paste,
                "pad_type": get_pad_type(pad),
                "shape": get_pad_shape(pad),
                "size": list(to_mm_2d(pad.GetSize())),
                "rotation": pad.GetOrientation().AsRadians(),
                "roundness": pad.GetRoundRectRadiusRatio(),
                "drill_shape": get_drill_shape(pad),
                "drill_size": list(to_mm_2d(pad.GetDrillSize())),
                "fab_type": get_pad_fab_type(pad),
            }
    
    # Create export result
    result = {
        "wrl_path": str(wrl_path),
        "components_dir": str(components_dir),
        "layers_dir": str(layers_dir),
        "bounds": bounds,
        "stackup": stackup,
        "boards": board_defs,
        "pads": pads
    }
    
    # Write result JSON
    result_path = output_dir / "export_result.json"
    with open(result_path, 'w') as f:
        json.dump(result, f, indent=2)
    
    return result

def main():
    if len(sys.argv) != 3:
        print("Usage: python export_pcb3d.py <pcb_file> <output_dir>", file=sys.stderr)
        sys.exit(1)
    
    pcb_path = Path(sys.argv[1])
    output_dir = Path(sys.argv[2])
    
    if not pcb_path.exists():
        print(f"Error: PCB file not found: {pcb_path}", file=sys.stderr)
        sys.exit(1)
    
    output_dir.mkdir(parents=True, exist_ok=True)
    
    try:
        export_pcb3d(pcb_path, output_dir)
    except Exception as e:
        print(f"Error exporting PCB3D: {e}", file=sys.stderr)
        import traceback
        traceback.print_exc()
        sys.exit(1)

if __name__ == "__main__":
    main()
