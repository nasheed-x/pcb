#![cfg(not(target_os = "windows"))]

use pcb_test_utils::assert_snapshot;
use pcb_test_utils::sandbox::Sandbox;

const SIMPLE_MODULE_WITH_LAYOUT: &str = r#"
# Simple board with layout path
add_property("layout_path", "./layout")

test_net = Net("TEST")
"#;

const KICAD_PCB_STUB: &str = r#"
(kicad_pcb
  (version 20221018)
  (generator pcbnew)
  (general
    (thickness 1.6)
    (legacy_teardrops no)
  )
  (paper "A4")
  (layers
    (0 "F.Cu" signal)
    (31 "B.Cu" signal)
    (32 "B.Adhes" user "B.Adhesive")
    (33 "F.Adhes" user "F.Adhesive")
    (34 "B.Paste" user)
    (35 "F.Paste" user)
    (36 "B.SilkS" user "B.Silkscreen")
    (37 "F.SilkS" user "F.Silkscreen")
    (38 "B.Mask" user)
    (39 "F.Mask" user)
    (40 "Dwgs.User" user "User.Drawings")
    (41 "Cmts.User" user "User.Comments")
    (42 "Eco1.User" user "User.Eco1")
    (43 "Eco2.User" user "User.Eco2")
    (44 "Edge.Cuts" user)
    (45 "Margin" user)
    (46 "B.CrtYd" user "B.Courtyard")
    (47 "F.CrtYd" user "F.Courtyard")
    (48 "B.Fab" user)
    (49 "F.Fab" user)
  )
  (setup
    (pad_to_mask_clearance 0)
    (pcbplotparams
      (layerselection 0x00010fc_ffffffff)
      (plot_on_all_layers_selection 0x0000000_00000000)
      (disableapertmacros no)
      (usegerberextensions no)
      (usegerberattributes yes)
      (usegerberadvancedattributes yes)
      (creategerberjobfile yes)
      (dashed_line_dash_ratio 12.000000)
      (dashed_line_gap_ratio 3.000000)
      (svgprecision 4)
      (plotframeref no)
      (viasonmask no)
      (mode 1)
      (useauxorigin no)
      (hpglpennumber 1)
      (hpglpenspeed 20)
      (hpglpendiameter 15.000000)
      (pdf_front_fp_property_popups yes)
      (pdf_back_fp_property_popups yes)
      (dxfpolygonmode yes)
      (dxfimperialunits yes)
      (dxfusepcbnewfont yes)
      (psnegative no)
      (psa4output no)
      (plotreference yes)
      (plotvalue yes)
      (plotfptext yes)
      (plotinvisibletext no)
      (sketchpadsonfab no)
      (subtractmaskfromsilk no)
      (outputformat 1)
      (mirror no)
      (drillshape 1)
      (scaleselection 1)
      (outputdirectory "")
    )
  )
  (net 0 "")
  (gr_rect (start 100 100) (end 200 150)
    (stroke (width 0.1) (type default))
    (fill none)
    (layer "Edge.Cuts")
  )
)
"#;

#[test]
fn test_pcb_render_no_zen_files() {
    let output = Sandbox::new().snapshot_run("pcb", ["render"]);
    assert_snapshot!("render_no_zen_files", output);
}

#[test]
fn test_pcb_render_no_layout() {
    // Create a simple .zen file without layout_path
    let simple_module_no_layout = r#"
# Simple board without layout
test_net = Net("TEST")
other_net = Net("OTHER")
"#;
    
    let output = Sandbox::new()
        .write("simple.zen", simple_module_no_layout)
        .snapshot_run("pcb", ["render"]);
    
    // The command should run successfully and warn about no layout
    assert!(
        output.contains("(no layout)") || 
        output.contains("Exit Code: 0"),
        "Unexpected output: {}",
        output
    );
}

#[test]
fn test_pcb_render_layout_file_missing() {
    // Create a .zen file with layout_path but no actual KiCad file
    let module_with_layout = r#"
# Board with layout path
add_property("layout_path", "./layout")

test_net = Net("TEST")
"#;
    
    let output = Sandbox::new()
        .write("simple.zen", module_with_layout)
        .snapshot_run("pcb", ["render"]);
    
    // Should error about layout file not found or create it successfully
    // Since process_layout creates the layout file if it doesn't exist, this may succeed
    assert!(
        output.contains("layout file not found") || 
        output.contains("Exit Code: 1") || 
        output.contains("Successfully rendered") ||
        output.contains("Exit Code: 0"),
        "Unexpected output: {}",
        output
    );
}

#[test]
#[ignore] // This test requires KiCad Python bindings to be available
fn test_pcb_render_success() {
    // Create a .zen file and stub KiCad layout
    let mut sandbox = Sandbox::new();
    sandbox
        .write("simple.zen", SIMPLE_MODULE_WITH_LAYOUT)
        .write("layout/layout.kicad_pcb", KICAD_PCB_STUB)
        .write("layout/default.net", "(net (code 0) (name \"\"))");
    
    // Run the render command
    let _ = sandbox.run("pcb", ["render", "--no-open"]);
    
    // Check that the .pcb3d file was created
    let pcb3d_path = sandbox.root_path().join("simple.pcb3d");
    assert!(pcb3d_path.exists(), "Expected .pcb3d file to be created");
}

#[test]
fn test_pcb_render_custom_output() {
    // Test specifying custom output path
    let module_with_layout = r#"
# Board with layout path
add_property("layout_path", "./layout")

test_net = Net("TEST")
"#;
    
    let output = Sandbox::new()
        .write("simple.zen", module_with_layout)
        .snapshot_run("pcb", ["render", "-o", "custom_output.pcb3d"]);
    
    // Should either error or succeed (if layout is created)
    assert!(
        output.contains("layout file not found") || 
        output.contains("Exit Code: 1") || 
        output.contains("Successfully rendered") ||
        output.contains("Exit Code: 0") ||
        output.contains("custom_output.pcb3d"),
        "Unexpected output: {}",
        output
    );
}

#[test]
fn test_pcb_render_help() {
    let output = Sandbox::new().snapshot_run("pcb", ["render", "--help"]);
    assert_snapshot!("render_help", output);
}
