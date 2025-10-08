//! PCB3D rendering functionality for 3D visualization
//! 
//! This crate provides functionality to convert KiCad PCB files to the PCB3D format
//! that can be imported into Blender using the pcb2blender addon.

use anyhow::{Context, Result};
use log::debug;
use pcb_layout::{process_layout, LayoutError};
use pcb_sch::Schematic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use zip::{write::FileOptions, ZipWriter};

/// PCB3D file structure constants
pub mod pcb3d {
    pub const PCB_WRL: &str = "pcb.wrl";
    pub const COMPONENTS_DIR: &str = "components";
    pub const LAYERS_DIR: &str = "layers";
    pub const BOARDS_DIR: &str = "boards";
    pub const PADS_DIR: &str = "pads";
    pub const BOUNDS_TOML: &str = "bounds.toml";
    pub const STACKUP_TOML: &str = "stackup.toml";
}

/// Bounds information for PCB and boards
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bounds {
    top_left: (f32, f32),
    size: (f32, f32),
}

impl Bounds {
    fn to_toml(&self) -> String {
        format!(
            "top_left = [ {:.6}, {:.6} ]\nsize = [ {:.6}, {:.6} ]",
            self.top_left.0, self.top_left.1, self.size.0, self.size.1
        )
    }
}

/// Stackup information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Stackup {
    thickness_mm: f32,
    mask_color: String,
    mask_color_custom: (f32, f32, f32),
    silks_color: String,
    silks_color_custom: (f32, f32, f32),
    surface_finish: String,
}

impl Default for Stackup {
    fn default() -> Self {
        Stackup {
            thickness_mm: 1.6,
            mask_color: "GREEN".to_string(),
            mask_color_custom: (0.0, 0.0, 0.0),
            silks_color: "WHITE".to_string(),
            silks_color_custom: (0.0, 0.0, 0.0),
            surface_finish: "HASL".to_string(),
        }
    }
}

impl Stackup {
    fn to_toml(&self) -> String {
        format!(
            r#"thickness_mm = {:.6}
mask_color = "{}"
mask_color_custom = [ {:.6}, {:.6}, {:.6} ]
silks_color = "{}"
silks_color_custom = [ {:.6}, {:.6}, {:.6} ]
surface_finish = "{}""#,
            self.thickness_mm,
            self.mask_color,
            self.mask_color_custom.0,
            self.mask_color_custom.1,
            self.mask_color_custom.2,
            self.silks_color,
            self.silks_color_custom.0,
            self.silks_color_custom.1,
            self.silks_color_custom.2,
            self.surface_finish
        )
    }
}

/// Pad types matching KiCad's pad attributes
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
enum PadType {
    Unknown = 255,
    Tht = 0,
    Smd = 1,
    Conn = 2,
    Npth = 3,
}

/// Pad shapes
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
enum PadShape {
    Unknown = 255,
    Circle = 0,
    Rect = 1,
    Oval = 2,
    Trapezoid = 3,
    RoundRect = 4,
    ChamferedRect = 5,
    Custom = 6,
}

/// Drill shapes
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
enum DrillShape {
    Unknown = 255,
    Circular = 0,
    Oval = 1,
}

/// Pad fabrication type
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
enum PadFabType {
    None = 0,
    Bga = 1,
    Fiducial = 2,
    TestPoint = 4,
    Heatsink = 5,
    Castellated = 6,
    Mechanical = 7,
}

/// Pad information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pad {
    position: (f32, f32),
    is_flipped: bool,
    has_model: bool,
    is_tht_or_smd: bool,
    has_paste: bool,
    pad_type: PadType,
    shape: PadShape,
    size: (f32, f32),
    rotation: f32,
    roundness: f32,
    drill_shape: DrillShape,
    drill_size: (f32, f32),
    fab_type: PadFabType,
}

impl Pad {
    fn to_toml(&self) -> String {
        format!(
            r#"position = [ {:.6}, {:.6} ]
is_flipped = {}
has_model = {}
is_tht_or_smd = {}
has_paste = {}
pad_type = "{:?}"
shape = "{:?}"
size = [ {:.6}, {:.6} ]
rotation = {:.6}
roundness = {:.6}
drill_shape = "{:?}"
drill_size = [ {:.6}, {:.6} ]
fab_type = "{:?}""#,
            self.position.0,
            self.position.1,
            self.is_flipped,
            self.has_model,
            self.is_tht_or_smd,
            self.has_paste,
            self.pad_type,
            self.shape,
            self.size.0,
            self.size.1,
            self.rotation,
            self.roundness,
            self.drill_shape,
            self.drill_size.0,
            self.drill_size.1,
            self.fab_type,
        )
    }
}

/// Board information
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Board {
    bounds: Bounds,
    stacked_boards: HashMap<String, StackedBoard>,
}

/// Stacked board offset information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StackedBoard {
    offset: (f32, f32, f32),
}

impl StackedBoard {
    fn to_toml(&self) -> String {
        format!(
            "offset = [ {:.6}, {:.6}, {:.6} ]",
            self.offset.0, self.offset.1, self.offset.2
        )
    }
}

/// Export result from Python script
#[derive(Debug, Deserialize)]
struct PythonExportResult {
    wrl_path: String,  // Keep as String for JSON deserialization
    components_dir: String,
    layers_dir: String,
    bounds: Bounds,
    stackup: Stackup,
    boards: HashMap<String, BoardExport>,
    pads: HashMap<String, Pad>,
}

#[derive(Debug, Deserialize)]
struct BoardExport {
    bounds: Bounds,
    stacked_boards: HashMap<String, StackedBoard>,
}

/// Options for rendering PCB to PCB3D format
pub struct RenderOptions {
    pub no_open: bool,
}

/// Result from processing a layout for rendering
pub struct LayoutRenderResult {
    pub source_file: PathBuf,
    pub pcb_file: PathBuf,
    pub output_path: PathBuf,
}

/// Process a schematic for rendering to PCB3D
pub fn process_for_render(schematic: &Schematic, zen_path: &Path) -> Result<PathBuf, LayoutError> {
    // Process layout to get the PCB file (with default sync_board_config = true)
    let layout_result = process_layout(schematic, zen_path, true)?;
    Ok(layout_result.pcb_file)
}

/// Render a KiCad PCB file to PCB3D format
pub fn render_pcb3d(pcb_file: &Path, output_path: &Path) -> Result<()> {
    debug!("Rendering {} to {}", pcb_file.display(), output_path.display());
    
    // Create a temporary directory for intermediate files
    let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
    
    // Call Python script to export from KiCad
    let export_result = call_kicad_export(pcb_file, temp_dir.path())?;
    
    // Create the PCB3D zip file  
    // Important: pass temp_dir to keep it alive during archive creation
    create_pcb3d_archive(output_path, &export_result, &temp_dir)?;
    
    // temp_dir will be cleaned up when it goes out of scope here
    Ok(())
}

/// Call the Python script to export KiCad PCB to intermediate formats
fn call_kicad_export(pcb_file: &Path, temp_dir: &Path) -> Result<PythonExportResult> {
    // Create Python script for KiCad export
    let script = create_export_script();
    let script_path = temp_dir.join("export_pcb3d.py");
    fs::write(&script_path, script)?;
    
    // Set up PYTHONPATH for KiCad Python modules
    let python_path = get_kicad_python_path();
    
    // Call the Python script
    let output = Command::new(get_python_interpreter())
        .env("PYTHONPATH", python_path)
        .arg(&script_path)
        .arg(pcb_file)
        .arg(temp_dir)
        .output()
        .context("Failed to execute Python export script")?;
    
    // Show stderr if there's important output (warnings, errors)
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() && (output.status.success() == false || std::env::var("PCB_RENDER_DEBUG").is_ok()) {
        eprintln!("Python script output:\n{}", stderr);
    }
    
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("Python stdout: {}", stdout);
        anyhow::bail!("Python export failed: {}", stderr);
    }
    
    // Parse the result JSON
    let result_path = temp_dir.join("export_result.json");
    if !result_path.exists() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("Python stdout: {}", stdout);
        anyhow::bail!("Export result file not created. Check Python output above.");
    }
    
    let result_json = fs::read_to_string(&result_path)
        .context("Failed to read export result")?;
    
    // Save JSON for debugging if requested
    if let Ok(debug_file) = std::env::var("PCB_RENDER_DEBUG") {
        let debug_path = if debug_file == "1" || debug_file == "true" {
            Path::new("/tmp/pcb_render_debug.json")
        } else {
            Path::new(&debug_file)
        };
        fs::write(&debug_path, &result_json)?;
        eprintln!("Debug JSON saved to: {:?}", debug_path);
    }
    
    let result: PythonExportResult = serde_json::from_str(&result_json)
        .with_context(|| {
            format!("Failed to parse export result. Set PCB_RENDER_DEBUG=1 to save debug JSON.")
        })?;
    
    Ok(result)
}

/// Get the Python interpreter path for KiCad
#[cfg(target_os = "macos")]
fn get_python_interpreter() -> String {
    std::env::var("KICAD_PYTHON_INTERPRETER").unwrap_or_else(|_|
        "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3".to_string())
}

#[cfg(target_os = "windows")]
fn get_python_interpreter() -> String {
    std::env::var("KICAD_PYTHON_INTERPRETER")
        .unwrap_or_else(|_| r"C:\Program Files\KiCad\9.0\bin\python.exe".to_string())
}

#[cfg(target_os = "linux")]
fn get_python_interpreter() -> String {
    std::env::var("KICAD_PYTHON_INTERPRETER").unwrap_or_else(|_| "/usr/bin/python3".to_string())
}

/// Get the PYTHONPATH for KiCad Python modules
#[cfg(target_os = "macos")]
fn get_kicad_python_path() -> String {
    let site_packages = std::env::var("KICAD_PYTHON_SITE_PACKAGES").unwrap_or_else(|_|
        "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages".to_string());
    site_packages
}

#[cfg(target_os = "windows")]
fn get_kicad_python_path() -> String {
    let site_packages = std::env::var("KICAD_PYTHON_SITE_PACKAGES")
        .unwrap_or_else(|_| {
            let home = dirs::home_dir().unwrap_or_default();
            home.join("Documents").join("KiCad").join("9.0").join("3rdparty").join("Python311").join("site-packages")
                .to_string_lossy().to_string()
        });
    site_packages
}

#[cfg(target_os = "linux")]
fn get_kicad_python_path() -> String {
    std::env::var("KICAD_PYTHON_SITE_PACKAGES")
        .unwrap_or_else(|_| "/usr/lib/python3/dist-packages".to_string())
}

/// Create the Python script for KiCad export operations
fn create_export_script() -> String {
    include_str!("scripts/kicad_export.py").to_string()
}

/// Create the final PCB3D archive from exported data
fn create_pcb3d_archive(output_path: &Path, export_result: &PythonExportResult, _temp_dir: &TempDir) -> Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let file = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated);
    
    // Write the main WRL file
    zip.start_file(pcb3d::PCB_WRL, options)?;
    let wrl_path = Path::new(&export_result.wrl_path);
    let wrl_content = fs::read(wrl_path)
        .with_context(|| format!("Failed to read WRL file from {:?}", wrl_path))?;
    zip.write_all(&wrl_content)?;
    
    // Create directories
    zip.add_directory(format!("{}/", pcb3d::COMPONENTS_DIR), options)?;
    zip.add_directory(format!("{}/", pcb3d::LAYERS_DIR), options)?;
    zip.add_directory(format!("{}/", pcb3d::BOARDS_DIR), options)?;
    zip.add_directory(format!("{}/", pcb3d::PADS_DIR), options)?;
    
    // Write component WRL files
    let components_dir = Path::new(&export_result.components_dir);
    for entry in fs::read_dir(components_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some(std::ffi::OsStr::new("wrl")) {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            zip.start_file(format!("{}/{}", pcb3d::COMPONENTS_DIR, file_name), options)?;
            let content = fs::read(&path)?;
            zip.write_all(&content)?;
        }
    }
    
    // Write layer SVG files
    let layers_dir = Path::new(&export_result.layers_dir);
    for entry in fs::read_dir(layers_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some(std::ffi::OsStr::new("svg")) {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            zip.start_file(format!("{}/{}", pcb3d::LAYERS_DIR, file_name), options)?;
            let content = fs::read(&path)?;
            zip.write_all(&content)?;
        }
    }
    
    // Write bounds and stackup
    zip.start_file(format!("{}/{}", pcb3d::LAYERS_DIR, pcb3d::BOUNDS_TOML), options)?;
    zip.write_all(export_result.bounds.to_toml().as_bytes())?;
    
    zip.start_file(format!("{}/{}", pcb3d::LAYERS_DIR, pcb3d::STACKUP_TOML), options)?;
    zip.write_all(export_result.stackup.to_toml().as_bytes())?;
    
    // Write board information
    for (board_name, board) in &export_result.boards {
        let board_dir = format!("{}/{}/", pcb3d::BOARDS_DIR, board_name);
        zip.add_directory(&board_dir, options)?;
        
        // Write board bounds
        zip.start_file(format!("{}{}", board_dir, pcb3d::BOUNDS_TOML), options)?;
        zip.write_all(board.bounds.to_toml().as_bytes())?;
        
        // Write stacked boards
        for (stacked_name, stacked) in &board.stacked_boards {
            zip.start_file(format!("{}stacked_{}.toml", board_dir, stacked_name), options)?;
            zip.write_all(stacked.to_toml().as_bytes())?;
        }
    }
    
    // Write pad information
    for (pad_name, pad) in &export_result.pads {
        zip.start_file(format!("{}/{}.toml", pcb3d::PADS_DIR, pad_name), options)?;
        zip.write_all(pad.to_toml().as_bytes())?;
    }
    
    zip.finish()?;
    Ok(())
}
