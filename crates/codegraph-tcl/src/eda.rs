// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! EDA command classification tables and extraction

use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

/// Classified EDA command result
#[allow(dead_code)]
pub enum EdaCommand {
    DesignFileRead {
        file_type: String,
        path: String,
    },
    DesignFileWrite {
        file_type: String,
        path: String,
    },
    ToolFlowCommand {
        name: String,
        category: String,
    },
    ObjectQuery {
        name: String,
        collection_type: String,
    },
    CommandRegistration {
        name: String,
        usage: String,
    },
    CollectionIteration {
        variable: String,
        collection_cmd: String,
    },
    AttributeAccess {
        object: String,
        attribute: String,
    },
}

/// Accumulated EDA data during visitation
#[derive(Debug, Default)]
pub struct EdaData {
    pub design_reads: Vec<(String, String)>,
    pub design_writes: Vec<(String, String)>,
    pub registered_commands: Vec<(String, String)>,
}

static DESIGN_READ_COMMANDS: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    HashMap::from([
        ("read_verilog", "verilog"),
        ("read_sverilog", "systemverilog"),
        ("read_vhdl", "vhdl"),
        ("read_liberty", "liberty"),
        ("read_lib", "liberty"),
        ("read_def", "def"),
        ("read_lef", "lef"),
        ("read_db", "db"),
        ("read_spef", "spef"),
        ("read_sdc", "sdc"),
        ("read_parasitics", "parasitics"),
        ("read_saif", "saif"),
        ("read_sdf", "sdf"),
        ("read_upf", "upf"),
        ("read_file", "generic"),
    ])
});

static DESIGN_WRITE_COMMANDS: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    HashMap::from([
        ("write_verilog", "verilog"),
        ("write_def", "def"),
        ("write_lef", "lef"),
        ("write_db", "db"),
        ("write_sdc", "sdc"),
        ("write_sdf", "sdf"),
        ("write_spef", "spef"),
        ("write", "generic"),
        ("write_file", "generic"),
        ("write_abstract_lef", "abstract_lef"),
        ("write_cdl", "cdl"),
    ])
});

static TOOL_FLOW_COMMANDS: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    HashMap::from([
        // Synthesis
        ("compile", "synthesis"),
        ("compile_ultra", "synthesis"),
        ("link", "synthesis"),
        ("link_design", "synthesis"),
        ("elaborate", "synthesis"),
        ("uniquify", "synthesis"),
        ("set_dont_use", "synthesis"),
        ("set_dont_touch", "synthesis"),
        // Floorplanning
        ("initialize_floorplan", "floorplan"),
        ("make_tracks", "floorplan"),
        // Placement
        ("global_placement", "placement"),
        ("detailed_placement", "placement"),
        ("legalize_placement", "placement"),
        ("set_placement_padding", "placement"),
        ("check_placement", "placement"),
        ("place_opt", "placement"),
        // CTS
        ("clock_tree_synthesis", "cts"),
        ("clock_opt", "cts"),
        ("repair_clock_nets", "cts"),
        ("repair_clock_inverters", "cts"),
        // Routing
        ("global_route", "routing"),
        ("detailed_route", "routing"),
        ("route_opt", "routing"),
        ("route_zrt_auto", "routing"),
        ("set_routing_layers", "routing"),
        ("set_global_routing_layer_adjustment", "routing"),
        // Timing
        ("report_timing", "timing"),
        ("report_checks", "timing"),
        ("report_tns", "timing"),
        ("report_wns", "timing"),
        ("report_worst_slack", "timing"),
        ("report_clock_min_period", "timing"),
        ("report_clock_skew", "timing"),
        ("repair_timing", "timing"),
        ("repair_design", "timing"),
        ("recover_power", "timing"),
        ("estimate_parasitics", "timing"),
        ("extract_parasitics", "timing"),
        // Power
        ("analyze_power_grid", "power"),
        ("report_power", "power"),
        ("set_pdnsim_net_voltage", "power"),
        // Physical
        ("add_global_connection", "physical"),
        ("global_connect", "physical"),
        ("repair_tie_fanout", "physical"),
        ("density_fill", "physical"),
        ("tapcell", "physical"),
        // Verification
        ("check_design", "verification"),
        ("report_constraint", "verification"),
        ("report_area", "verification"),
        ("report_qor", "verification"),
    ])
});

static OBJECT_QUERY_COMMANDS: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    HashMap::from([
        ("get_cells", "cell"),
        ("get_pins", "pin"),
        ("get_ports", "port"),
        ("get_nets", "net"),
        ("get_clocks", "clock"),
        ("get_lib_cells", "lib_cell"),
        ("get_lib_pins", "lib_pin"),
        ("get_registers", "register"),
        ("all_clocks", "clock"),
        ("all_inputs", "port"),
        ("all_outputs", "port"),
        ("all_registers", "register"),
        ("current_design", "design"),
        ("current_instance", "instance"),
    ])
});

/// OpenROAD namespace prefixes
const OPENROAD_PREFIXES: &[&str] = &[
    "sta::", "ord::", "gpl::", "cts::", "drt::", "rcx::", "pdn::", "rsz::", "par::", "ppl::",
    "tap::", "grt::", "mpl::", "rmp::", "psm::", "utl::",
];

pub fn is_openroad_namespaced(name: &str) -> bool {
    OPENROAD_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Strip namespace prefix from a command name for lookup
fn base_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

pub fn is_eda_command(name: &str) -> bool {
    let base = base_name(name);
    DESIGN_READ_COMMANDS.contains_key(base)
        || DESIGN_WRITE_COMMANDS.contains_key(base)
        || TOOL_FLOW_COMMANDS.contains_key(base)
        || OBJECT_QUERY_COMMANDS.contains_key(base)
        || base == "define_cmd_args"
        || base == "get_attribute"
        || base == "set_attribute"
        || base == "foreach_in_collection"
        || base == "sizeof_collection"
        || base == "add_to_collection"
        || base == "remove_from_collection"
        || base == "filter_collection"
        || base == "sort_collection"
        || base == "index_collection"
        || is_openroad_namespaced(name)
}

/// Classify an EDA command and extract structured data from its arguments
pub fn classify_eda_command(name: &str, node: Node, source: &[u8]) -> Option<EdaCommand> {
    let args = collect_positional_args(node, source, true);
    classify_eda_from_args(name, args)
}

/// Classify an EDA command from a split ERROR+command pair.
/// Does not skip the first child since the command name was in the ERROR node.
pub fn classify_eda_command_from_split(
    name: &str,
    node: Node,
    source: &[u8],
) -> Option<EdaCommand> {
    let args = collect_positional_args(node, source, false);
    classify_eda_from_args(name, args)
}

fn classify_eda_from_args(name: &str, args: Vec<String>) -> Option<EdaCommand> {
    let base = base_name(name);

    if let Some(&file_type) = DESIGN_READ_COMMANDS.get(base) {
        let path = find_file_argument(&args);
        return Some(EdaCommand::DesignFileRead {
            file_type: file_type.to_string(),
            path,
        });
    }

    if let Some(&file_type) = DESIGN_WRITE_COMMANDS.get(base) {
        let path = find_file_argument(&args);
        return Some(EdaCommand::DesignFileWrite {
            file_type: file_type.to_string(),
            path,
        });
    }

    if let Some(&category) = TOOL_FLOW_COMMANDS.get(base) {
        return Some(EdaCommand::ToolFlowCommand {
            name: name.to_string(),
            category: category.to_string(),
        });
    }

    if let Some(&collection_type) = OBJECT_QUERY_COMMANDS.get(base) {
        return Some(EdaCommand::ObjectQuery {
            name: name.to_string(),
            collection_type: collection_type.to_string(),
        });
    }

    match base {
        "define_cmd_args" => {
            let cmd_name = args.first().cloned().unwrap_or_default();
            let usage = args.get(1).cloned().unwrap_or_default();
            Some(EdaCommand::CommandRegistration {
                name: cmd_name.trim_matches('"').to_string(),
                usage: usage.trim_matches('{').trim_matches('}').to_string(),
            })
        }
        "foreach_in_collection" => {
            let variable = args.first().cloned().unwrap_or_default();
            let collection_cmd = args.get(1).cloned().unwrap_or_default();
            Some(EdaCommand::CollectionIteration {
                variable,
                collection_cmd,
            })
        }
        "get_attribute" | "set_attribute" => {
            let object = args.first().cloned().unwrap_or_default();
            let attribute = args.get(1).cloned().unwrap_or_default();
            Some(EdaCommand::AttributeAccess { object, attribute })
        }
        _ => {
            if is_openroad_namespaced(name) {
                Some(EdaCommand::ToolFlowCommand {
                    name: name.to_string(),
                    category: "openroad".to_string(),
                })
            } else {
                None
            }
        }
    }
}

/// Extract positional arguments from a command node (skip flags starting with -)
fn collect_positional_args(node: Node, source: &[u8], skip_first: bool) -> Vec<String> {
    let mut args = Vec::new();
    let mut cursor = node.walk();
    let mut skipped_name = !skip_first;
    let mut skip_next = false;

    for child in node.children(&mut cursor) {
        let text = child.utf8_text(source).unwrap_or("").trim().to_string();

        if !skipped_name {
            skipped_name = true;
            continue;
        }

        if skip_next {
            skip_next = false;
            continue;
        }

        if text.is_empty() {
            continue;
        }

        args.push(text);
    }
    args
}

/// Find the first argument that looks like a file path (doesn't start with -)
fn find_file_argument(args: &[String]) -> String {
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with('-') {
            // Flag with value: skip the next arg too
            skip_next = true;
            continue;
        }
        // This is a positional argument - likely a file path
        return arg.trim_matches('"').trim_matches('\'').to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_eda_command_design_reads() {
        assert!(is_eda_command("read_verilog"));
        assert!(is_eda_command("read_liberty"));
        assert!(is_eda_command("read_def"));
        assert!(is_eda_command("read_lef"));
    }

    #[test]
    fn test_is_eda_command_design_writes() {
        assert!(is_eda_command("write_verilog"));
        assert!(is_eda_command("write_def"));
    }

    #[test]
    fn test_is_eda_command_tool_flow() {
        assert!(is_eda_command("compile"));
        assert!(is_eda_command("global_placement"));
        assert!(is_eda_command("report_timing"));
    }

    #[test]
    fn test_is_eda_command_object_queries() {
        assert!(is_eda_command("get_cells"));
        assert!(is_eda_command("get_pins"));
        assert!(is_eda_command("all_clocks"));
    }

    #[test]
    fn test_is_eda_command_synopsys() {
        assert!(is_eda_command("foreach_in_collection"));
        assert!(is_eda_command("get_attribute"));
        assert!(is_eda_command("define_cmd_args"));
    }

    #[test]
    fn test_is_eda_command_negative() {
        assert!(!is_eda_command("proc"));
        assert!(!is_eda_command("set"));
        assert!(!is_eda_command("foreach"));
        assert!(!is_eda_command("puts"));
    }

    #[test]
    fn test_is_openroad_namespaced() {
        assert!(is_openroad_namespaced("sta::parse_key_args"));
        assert!(is_openroad_namespaced("ord::read_lef_cmd"));
        assert!(is_openroad_namespaced(
            "gpl::get_global_placement_uniform_density"
        ));
        assert!(!is_openroad_namespaced("my_proc"));
    }

    #[test]
    fn test_base_name_stripping() {
        assert_eq!(base_name("sta::create_clock"), "create_clock");
        assert_eq!(base_name("ord::read_lef_cmd"), "read_lef_cmd");
        assert_eq!(base_name("compile"), "compile");
    }

    #[test]
    fn test_namespaced_eda_commands() {
        assert!(is_eda_command("sta::report_timing"));
        assert!(is_eda_command("ord::read_verilog"));
    }

    #[test]
    fn test_find_file_argument() {
        let args = vec![
            "-format".to_string(),
            "verilog".to_string(),
            "design.v".to_string(),
        ];
        assert_eq!(find_file_argument(&args), "design.v");

        let args2 = vec!["design.v".to_string()];
        assert_eq!(find_file_argument(&args2), "design.v");

        let empty: Vec<String> = vec![];
        assert_eq!(find_file_argument(&empty), "");
    }
}
