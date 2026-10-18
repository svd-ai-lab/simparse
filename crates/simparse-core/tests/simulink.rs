use std::io::Write;

use simparse_core::{
    CompactFormatSummary, FormatSummary, InspectOptions, ScanOptions, SimFormat, detect_format,
    inspect_path, scan_paths, summarize_result,
};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

const BLOCKDIAGRAM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ModelInformation Version="1.0">
  <Model>
    <UserParameters>
      <P Name="PhysicalModelingProducts">Simscape|SimMechanics</P>
    </UserParameters>
    <System Ref="system_root"/>
  </Model>
</ModelInformation>"#;

const SYSTEM_ROOT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<System>
  <P Name="Location">[0, 0, 100, 100]</P>
  <Block BlockType="Inport" Name="second" SID="1"><P Name="Port">2</P></Block>
  <Block BlockType="Inport" Name="first" SID="2"/>
  <Block BlockType="Outport" Name="y" SID="3"/>
  <Block BlockType="Gain" Name="k" SID="4"><P Name="Gain">K</P></Block>
  <Block BlockType="SubSystem" Name="plant" SID="5"><System Ref="system_5"/></Block>
  <Block BlockType="SubSystem" Name="logic" SID="6">
    <P Name="SFBlockType">Chart</P>
    <System Ref="system_6"/>
  </Block>
  <Block BlockType="SubSystem" Name="f" SID="7">
    <P Name="SFBlockType">MATLAB Function</P>
    <System Ref="system_7"/>
  </Block>
  <Block BlockType="Reference" Name="pid" SID="8">
    <P Name="SourceBlock">simulink/Continuous/PID&#xA;Controller &amp; more</P>
    <InstanceData><P Name="SourceBlock">ignored/nested</P></InstanceData>
  </Block>
  <Block BlockType="ModelReference" Name="ref" SID="9"><P Name="ModelName">child_model</P></Block>
  <Line><P Name="Src">1#out:1</P><Branch><P Name="Dst">4#in:1</P></Branch></Line>
  <Line><P Name="Src">4#out:1</P></Line>
</System>"#;

const SYSTEM_PLANT: &str = r#"<System>
  <Block BlockType="Integrator" Name="x" SID="10"/>
  <Block BlockType="SubSystem" Name="inner" SID="11"><System Ref="system_11"/></Block>
  <Line/>
</System>"#;

const SYSTEM_INNER: &str = r#"<System><Block BlockType="Sum" Name="s" SID="12"/></System>"#;

const SYSTEM_GENERATED: &str = r#"<System>
  <Block BlockType="S-Function" Name=" SFunction " SID="20"/>
  <Block BlockType="Demux" Name=" Demux " SID="21"/>
  <Line/>
</System>"#;

const CHART: &str = r#"<chart id="30">
  <P Name="name">logic</P>
  <Children>
    <state SSID="1"><P Name="labelString">A</P></state>
    <state SSID="2"><P Name="labelString">B</P></state>
    <transition SSID="3"/>
    <data SSID="4" name="u"/>
  </Children>
</chart>"#;

const EML_CHART: &str = r#"<chart id="40">
  <P Name="type">EML_CHART</P>
  <Children>
    <state SSID="1"/>
    <data SSID="2" name="u"/>
    <data SSID="3" name="y"/>
  </Children>
</chart>"#;

fn config_set(solver: &str, stop: &str) -> String {
    format!(
        r#"<ConfigSet><Object ObjectID="1" ClassName="Simulink.ConfigSet"><Array PropName="Components">
<Object ObjectID="2" ClassName="Simulink.SolverCC">
  <P Name="StartTime" Class="char">0.0</P>
  <P Name="StopTime" Class="char">{stop}</P>
  <P Name="FixedStep" Class="char">0.01</P>
  <P Name="SolverName" Class="char">{solver}</P>
</Object>
<Object ObjectID="3" ClassName="Simulink.DataIOCC"><P Name="StopTime" Class="char">999</P></Object>
</Array></Object></ConfigSet>"#
    )
}

fn write_slx(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    let parts = [
        ("simulink/blockdiagram.xml", BLOCKDIAGRAM.to_string()),
        ("simulink/systems/system_root.xml", SYSTEM_ROOT.to_string()),
        ("simulink/systems/system_5.xml", SYSTEM_PLANT.to_string()),
        ("simulink/systems/system_11.xml", SYSTEM_INNER.to_string()),
        ("simulink/systems/system_6.xml", SYSTEM_GENERATED.to_string()),
        ("simulink/systems/system_7.xml", SYSTEM_GENERATED.to_string()),
        ("simulink/stateflow/chart_30.xml", CHART.to_string()),
        ("simulink/stateflow/chart_40.xml", EML_CHART.to_string()),
        (
            "simulink/stateflow/machine.xml",
            r#"<Stateflow><machine id="1"><Children><chart Ref="chart_30"/><chart Ref="chart_40"/></Children></machine></Stateflow>"#.to_string(),
        ),
        (
            "simulink/configSetInfo.xml",
            r#"<ConfigSetInfo><ConfigSet PartName="/simulink/configSet0.xml">Default</ConfigSet><ConfigSet PartName="/simulink/configSet1.xml" Active="true">Fast</ConfigSet></ConfigSetInfo>"#.to_string(),
        ),
        ("simulink/configSet0.xml", config_set("ode45", "10")),
        ("simulink/configSet1.xml", config_set("ode4", "20")),
        (
            "metadata/mwcorePropertiesReleaseInfo.xml",
            "<MathWorks_version_info><version>99.1.0</version><release>R2099a</release></MathWorks_version_info>".to_string(),
        ),
        (
            "metadata/coreProperties.xml",
            r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="y"><dc:creator>secret-author</dc:creator><cp:version>R2099a</cp:version></cp:coreProperties>"#.to_string(),
        ),
        ("simulink/modelWorkspace.mxarray", "binary".to_string()),
    ];
    for (name, text) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn inspects_split_slx_package() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("model.slx");
    write_slx(&path);

    assert_eq!(detect_format(&path), Some(SimFormat::SimulinkSlx));
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    assert!(
        !json.contains("secret-author"),
        "author fields must not be emitted"
    );

    let FormatSummary::Simulink(s) = &result.summary else {
        panic!("expected Simulink summary");
    };
    assert_eq!(s.container, "slx");
    assert_eq!(s.kind.as_deref(), Some("model"));
    assert_eq!(s.saved_release.as_deref(), Some("R2099a"));
    assert_eq!(s.block_count, 12);
    assert_eq!(s.subsystem_count, 2);
    assert_eq!(s.max_depth, 2);
    assert_eq!(s.line_count, 3);
    let count = |name: &str| {
        s.block_types
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.count)
    };
    assert_eq!(count("Chart"), Some(1));
    assert_eq!(count("MATLAB Function"), Some(1));
    assert_eq!(count("S-Function"), None);
    assert_eq!(
        s.library_links[0].name,
        "simulink/Continuous/PID Controller & more"
    );
    assert_eq!(s.libraries[0].name, "simulink");
    assert_eq!(s.model_references, vec!["child_model"]);
    assert_eq!(s.inports, vec!["first", "second"]);
    assert_eq!(s.outports, vec!["y"]);
    assert_eq!(
        s.physical_modeling_products,
        vec!["Simscape", "SimMechanics"]
    );
    assert_eq!(s.stateflow.charts, 1);
    assert_eq!(s.stateflow.states, 2);
    assert_eq!(s.stateflow.transitions, 1);
    assert_eq!(s.stateflow.data, 1);
    assert_eq!(s.solver.solver.as_deref(), Some("ode4"));
    assert_eq!(s.solver.solver_type.as_deref(), Some("Fixed-step"));
    assert_eq!(s.solver.stop_time.as_deref(), Some("20"));
    assert!(s.model_workspace);

    let summary = summarize_result(&result);
    let CompactFormatSummary::Simulink(c) = &summary.summary else {
        panic!("expected compact Simulink summary");
    };
    assert_eq!(c.block_types.total, s.block_types.len());
    assert!(!summary.limitations.is_empty());
    let ir = result.ir.as_ref().unwrap();
    let attrs = &ir["dialects"]["cae"]["extensions"]["mathworks.simulink"]["attributes"];
    assert_eq!(attrs["block_count"], 12);
    assert_eq!(attrs["solver"], "ode4");
}

const CLASSIC_MDL: &str = r#"# comment
Model {
  Name			  "classic"
  Version		  10.4
  PhysicalModelingProducts "Simscape"
  Array {
    Type		    "Handle"
    Simulink.ConfigSet {
      Simulink.SolverCC {
        StartTime	"0.0"
        StopTime	"20"
        SolverName	"ode45"
      }
    }
  }
  BlockParameterDefaults {
    Block {
      BlockType	Gain
    }
  }
  System {
    Name		    "classic"
    Block {
      BlockType		      Inport
      Name		      "u"
    }
    Block {
      BlockType		      Gain
      Name		      "K"
    }
    Block {
      BlockType		      SubSystem
      Name		      "sub"
      System {
        Name		"sub"
        Block {
          BlockType	Integrator
          Name		"x"
        }
        Line {
          SrcBlock	"x"
        }
      }
    }
    Block {
      BlockType		      SubSystem
      Name		      "mf"
      SFBlockType	      "MATLAB Function"
      System {
        Block {
          BlockType	S-Function
          Name		" SFunction "
        }
      }
    }
    Line {
      SrcBlock		      "u"
      Branch {
        DstBlock		      "K"
      }
    }
  }
}
Stateflow {
  machine {
    id			  1
  }
  chart {
    id			  2
    name		  "c"
  }
  chart {
    id			  5
    type		  EML_CHART
  }
  state {
    id			  3
    chart		  2
  }
  state {
    id			  6
    chart		  5
  }
  transition {
    id			  4
    chart		  2
    src {
      id		  3
    }
  }
  data {
    id			  7
    linkNode		  [2 0 0]
  }
  data {
    id			  8
    linkNode		  [5 0 0]
  }
}
"#;

#[test]
fn inspects_classic_mdl_text() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("classic.mdl");
    std::fs::write(&path, CLASSIC_MDL).unwrap();

    assert_eq!(detect_format(&path), Some(SimFormat::SimulinkMdl));
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::Simulink(s) = &result.summary else {
        panic!("expected Simulink summary");
    };
    assert_eq!(s.container, "mdl");
    assert_eq!(s.kind.as_deref(), Some("model"));
    assert_eq!(s.saved_version.as_deref(), Some("10.4"));
    assert_eq!(s.block_count, 5);
    assert_eq!(s.subsystem_count, 1);
    assert_eq!(s.max_depth, 1);
    assert_eq!(s.line_count, 2);
    assert_eq!(s.inports, vec!["u"]);
    assert_eq!(s.stateflow_blocks[0].name, "MATLAB Function");
    assert_eq!(s.solver.solver.as_deref(), Some("ode45"));
    assert_eq!(s.solver.solver_type.as_deref(), Some("Variable-step"));
    assert_eq!(s.solver.stop_time.as_deref(), Some("20"));
    assert_eq!(s.physical_modeling_products, vec!["Simscape"]);
    assert_eq!(s.stateflow.charts, 1);
    assert_eq!(s.stateflow.states, 1);
    assert_eq!(s.stateflow.transitions, 1);
    assert_eq!(s.stateflow.data, 1);
}

#[test]
fn inspects_text_packaged_mdl() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("packaged.mdl");
    let text = format!(
        "# MathWorks OPC Text Package\n__MWOPC_PACKAGE_BEGIN__ R2099a\n__MWOPC_PART_BEGIN__ /simulink/blockdiagram.xml\n{BLOCKDIAGRAM}\n__MWOPC_PART_BEGIN__ /simulink/systems/system_root.xml\n{SYSTEM_INNER}\n__MWOPC_PACKAGE_END\n"
    );
    std::fs::write(&path, text).unwrap();

    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::Simulink(s) = &result.summary else {
        panic!("expected Simulink summary");
    };
    assert_eq!(s.container, "mdl-opc");
    assert_eq!(s.block_count, 1);
    assert_eq!(s.block_types[0].name, "Sum");
}

#[test]
fn scans_simulink_models_by_default() {
    let tmp = tempdir().unwrap();
    write_slx(&tmp.path().join("a.slx"));
    std::fs::write(tmp.path().join("b.mdl"), CLASSIC_MDL).unwrap();
    std::fs::write(tmp.path().join("._a.slx"), b"\x00\x05\x16\x07 AppleDouble").unwrap();
    std::fs::write(tmp.path().join("empty.slx"), b"").unwrap();
    std::fs::write(tmp.path().join("other.mdl"), "<<version=1>>\n<<info>>\n").unwrap();
    let results = scan_paths(&[tmp.path().to_path_buf()], ScanOptions::default()).unwrap();
    let mut formats: Vec<_> = results.iter().map(|r| r.format).collect();
    formats.sort_by_key(|f| f.to_string());
    assert_eq!(
        formats,
        vec![SimFormat::SimulinkMdl, SimFormat::SimulinkSlx]
    );
}

const INLINE_BLOCKDIAGRAM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ModelInformation Version="1.0">
  <Library>
    <Array PropName="ConfigurationSets">
      <Object ObjectID="1" ClassName="Simulink.ConfigSet">
        <Object ObjectID="2" ClassName="Simulink.SolverCC">
          <P Name="SolverName">ode45</P>
        </Object>
      </Object>
      <Object ObjectID="3" ClassName="Simulink.ConfigSetRef">
        <P Name="WSVarName">SharedConfig</P>
        <P Name="SourceLocation">Data Dictionary</P>
      </Object>
    </Array>
    <Object Reference="true" PropName="ActiveConfigurationSet" ObjectID="3" ClassName="Simulink.ConfigSetRef"/>
    <System>
      <Block BlockType="Outport" Name="out" SID="1"/>
      <Block BlockType="SubSystem" Name="s" SID="2">
        <System>
          <Block BlockType="Gain" Name="g" SID="3"/>
          <Line/>
        </System>
      </Block>
      <Line/>
    </System>
  </Library>
</ModelInformation>"#;

#[test]
fn inspects_inline_slx_with_referenced_configuration() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("old.slx");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("simulink/blockdiagram.xml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(INLINE_BLOCKDIAGRAM.as_bytes()).unwrap();
    zip.finish().unwrap();

    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::Simulink(s) = &result.summary else {
        panic!("expected Simulink summary");
    };
    assert_eq!(s.kind.as_deref(), Some("library"));
    assert_eq!(s.block_count, 3);
    assert_eq!(s.subsystem_count, 1);
    assert_eq!(s.max_depth, 1);
    assert_eq!(s.line_count, 2);
    assert_eq!(s.outports, vec!["out"]);
    assert_eq!(s.solver.solver, None);
    assert_eq!(s.solver.reference.as_deref(), Some("SharedConfig"));
    assert_eq!(
        s.solver.reference_location.as_deref(),
        Some("Data Dictionary")
    );
}
