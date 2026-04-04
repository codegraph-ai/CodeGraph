// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! SDC constraint types and extraction

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

/// Structured SDC clock definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdcClock {
    pub name: String,
    pub period: String,
    pub port: String,
}

/// Structured SDC IO delay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdcIoDelay {
    pub delay_type: String,
    pub clock: String,
    pub delay: String,
}

/// Structured SDC timing exception
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdcTimingException {
    pub exception_type: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub value: Option<String>,
}

/// Accumulated SDC data during parsing
#[derive(Debug, Default)]
pub struct SdcData {
    pub clocks: Vec<SdcClock>,
    pub io_delays: Vec<SdcIoDelay>,
    pub timing_exceptions: Vec<SdcTimingException>,
}

pub enum SdcConstraint {
    Clock(SdcClock),
    IoDelay(SdcIoDelay),
    TimingException(SdcTimingException),
}

impl SdcData {
    pub fn add(&mut self, constraint: SdcConstraint) {
        match constraint {
            SdcConstraint::Clock(c) => self.clocks.push(c),
            SdcConstraint::IoDelay(d) => self.io_delays.push(d),
            SdcConstraint::TimingException(e) => self.timing_exceptions.push(e),
        }
    }
}

const SDC_COMMANDS: &[&str] = &[
    "create_clock",
    "create_generated_clock",
    "set_input_delay",
    "set_output_delay",
    "set_false_path",
    "set_multicycle_path",
    "set_max_delay",
    "set_min_delay",
    "set_clock_uncertainty",
    "set_clock_latency",
    "set_clock_groups",
    "set_max_fanout",
    "set_max_transition",
    "set_max_capacitance",
    "set_load",
    "set_driving_cell",
    "set_input_transition",
    "set_propagated_clock",
    "group_path",
];

pub fn is_sdc_command(name: &str) -> bool {
    SDC_COMMANDS.contains(&name)
}

/// Collect argument texts from a command node's children.
/// Skips the command name (first word) and returns remaining words as strings.
pub fn collect_args(node: Node, source: &[u8]) -> Vec<String> {
    collect_args_inner(node, source, true)
}

/// Collect all args from a command node without skipping the first child.
/// Used when the command name was already extracted from a sibling ERROR node
/// (the grammar's position-0 split pattern).
pub fn collect_args_from_split(node: Node, source: &[u8]) -> Vec<String> {
    collect_args_inner(node, source, false)
}

fn collect_args_inner(node: Node, source: &[u8], skip_first: bool) -> Vec<String> {
    let mut args = Vec::new();
    let mut cursor = node.walk();
    let mut skipped_name = !skip_first;

    for child in node.children(&mut cursor) {
        match child.kind() {
            "simple_word"
            | "quoted_word"
            | "braced_word"
            | "braced_word_simple"
            | "variable_substitution"
            | "command_substitution"
            | "word_list" => {
                if !skipped_name {
                    skipped_name = true;
                    continue;
                }
                let text = child.utf8_text(source).unwrap_or("").to_string();
                // Flatten word_list children
                if child.kind() == "word_list" {
                    let mut wl_cursor = child.walk();
                    for wl_child in child.children(&mut wl_cursor) {
                        let wl_text = wl_child.utf8_text(source).unwrap_or("").trim().to_string();
                        if !wl_text.is_empty() {
                            args.push(wl_text);
                        }
                    }
                } else {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        args.push(trimmed);
                    }
                }
            }
            _ => {
                if !skipped_name {
                    skipped_name = true;
                }
            }
        }
    }
    args
}

/// Parse an SDC command node into a structured constraint
pub fn extract_sdc_constraint(cmd_name: &str, node: Node, source: &[u8]) -> Option<SdcConstraint> {
    let args = collect_args(node, source);
    extract_sdc_from_args(cmd_name, &args)
}

/// Parse an SDC constraint from a split ERROR+command pair.
/// Does not skip the first child of the command node since the command name
/// was already extracted from the sibling ERROR node.
pub fn extract_sdc_constraint_from_split(
    cmd_name: &str,
    node: Node,
    source: &[u8],
) -> Option<SdcConstraint> {
    let args = collect_args_from_split(node, source);
    extract_sdc_from_args(cmd_name, &args)
}

pub fn extract_sdc_from_args(cmd_name: &str, args: &[String]) -> Option<SdcConstraint> {
    match cmd_name {
        "create_clock" | "create_generated_clock" => extract_create_clock(args),
        "set_input_delay" => extract_io_delay("input", args),
        "set_output_delay" => extract_io_delay("output", args),
        "set_false_path" => extract_timing_exception("false_path", args),
        "set_multicycle_path" => extract_timing_exception("multicycle_path", args),
        "set_max_delay" => extract_timing_exception("max_delay", args),
        "set_min_delay" => extract_timing_exception("min_delay", args),
        "set_clock_uncertainty" => extract_timing_exception("clock_uncertainty", args),
        "set_clock_latency" => extract_timing_exception("clock_latency", args),
        "set_clock_groups" => extract_timing_exception("clock_groups", args),
        _ => None,
    }
}

fn extract_create_clock(args: &[String]) -> Option<SdcConstraint> {
    let mut name = String::new();
    let mut period = String::new();
    let mut port = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-name" => {
                name = args.get(i + 1).cloned().unwrap_or_default();
                i += 2;
            }
            "-period" => {
                period = args.get(i + 1).cloned().unwrap_or_default();
                i += 2;
            }
            "-waveform" | "-source" | "-divide_by" | "-multiply_by" | "-duty_cycle"
            | "-combinational" | "-add" | "-master_clock" | "-invert" | "-edge_shift"
            | "-edges" => {
                // Skip flag + value
                i += 2;
            }
            s if s.starts_with('-') => {
                i += 1;
            }
            s => {
                // Positional: could be [get_ports ...] or a port name
                if s.contains("get_ports") {
                    port = extract_port_from_bracket(s);
                } else if port.is_empty() {
                    port = s.to_string();
                }
                i += 1;
            }
        }
    }

    Some(SdcConstraint::Clock(SdcClock { name, period, port }))
}

fn extract_io_delay(delay_type: &str, args: &[String]) -> Option<SdcConstraint> {
    let mut clock = String::new();
    let mut delay = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-clock" => {
                clock = args.get(i + 1).cloned().unwrap_or_default();
                i += 2;
            }
            "-max"
            | "-min"
            | "-rise"
            | "-fall"
            | "-add_delay"
            | "-network_latency_included"
            | "-source_latency_included"
            | "-reference_pin" => {
                i += 2;
            }
            s if s.starts_with('-') => {
                i += 1;
            }
            s => {
                // First positional is the delay value, rest are port specs
                if delay.is_empty() {
                    delay = s.to_string();
                }
                i += 1;
            }
        }
    }

    Some(SdcConstraint::IoDelay(SdcIoDelay {
        delay_type: delay_type.to_string(),
        clock,
        delay,
    }))
}

fn extract_timing_exception(exception_type: &str, args: &[String]) -> Option<SdcConstraint> {
    let mut from = None;
    let mut to = None;
    let mut value = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-from" | "-rise_from" | "-fall_from" => {
                from = args.get(i + 1).cloned();
                i += 2;
            }
            "-to" | "-rise_to" | "-fall_to" => {
                to = args.get(i + 1).cloned();
                i += 2;
            }
            "-through" | "-rise_through" | "-fall_through" => {
                i += 2;
            }
            "-setup"
            | "-hold"
            | "-comment"
            | "-group"
            | "-name"
            | "-physically_exclusive"
            | "-logically_exclusive"
            | "-asynchronous"
            | "-allow_paths" => {
                i += 2;
            }
            s if s.starts_with('-') => {
                i += 1;
            }
            s => {
                // Positional value (e.g., multiplier for multicycle, delay value for max_delay)
                if value.is_none() {
                    value = Some(s.to_string());
                }
                i += 1;
            }
        }
    }

    Some(SdcConstraint::TimingException(SdcTimingException {
        exception_type: exception_type.to_string(),
        from,
        to,
        value,
    }))
}

fn extract_port_from_bracket(s: &str) -> String {
    // Extract port name from patterns like "[get_ports clk]" or "[get_ports {clk}]"
    let trimmed = s.trim_start_matches('[').trim_end_matches(']');
    if let Some(rest) = trimmed.strip_prefix("get_ports") {
        let port = rest.trim().trim_matches('{').trim_matches('}').trim();
        return port.to_string();
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sdc_command() {
        assert!(is_sdc_command("create_clock"));
        assert!(is_sdc_command("set_input_delay"));
        assert!(is_sdc_command("set_false_path"));
        assert!(!is_sdc_command("read_verilog"));
        assert!(!is_sdc_command("proc"));
    }

    #[test]
    fn test_extract_create_clock() {
        let args = vec![
            "-name".to_string(),
            "clk".to_string(),
            "-period".to_string(),
            "10".to_string(),
            "[get_ports clk_in]".to_string(),
        ];
        let result = extract_create_clock(&args);
        assert!(result.is_some());
        if let Some(SdcConstraint::Clock(clock)) = result {
            assert_eq!(clock.name, "clk");
            assert_eq!(clock.period, "10");
            assert_eq!(clock.port, "clk_in");
        }
    }

    #[test]
    fn test_extract_io_delay() {
        let args = vec!["-clock".to_string(), "clk".to_string(), "0.5".to_string()];
        let result = extract_io_delay("input", &args);
        assert!(result.is_some());
        if let Some(SdcConstraint::IoDelay(delay)) = result {
            assert_eq!(delay.delay_type, "input");
            assert_eq!(delay.clock, "clk");
            assert_eq!(delay.delay, "0.5");
        }
    }

    #[test]
    fn test_extract_timing_exception() {
        let args = vec![
            "-from".to_string(),
            "[get_clocks clk1]".to_string(),
            "-to".to_string(),
            "[get_clocks clk2]".to_string(),
        ];
        let result = extract_timing_exception("false_path", &args);
        assert!(result.is_some());
        if let Some(SdcConstraint::TimingException(exc)) = result {
            assert_eq!(exc.exception_type, "false_path");
            assert_eq!(exc.from.unwrap(), "[get_clocks clk1]");
            assert_eq!(exc.to.unwrap(), "[get_clocks clk2]");
        }
    }

    #[test]
    fn test_sdc_data_accumulation() {
        let mut data = SdcData::default();
        assert!(data.clocks.is_empty());

        data.add(SdcConstraint::Clock(SdcClock {
            name: "clk".to_string(),
            period: "10".to_string(),
            port: "clk_in".to_string(),
        }));
        assert!(!data.clocks.is_empty());
        assert_eq!(data.clocks.len(), 1);
    }

    #[test]
    fn test_extract_port_from_bracket() {
        assert_eq!(extract_port_from_bracket("[get_ports clk]"), "clk");
        assert_eq!(extract_port_from_bracket("[get_ports {clk}]"), "clk");
        assert_eq!(extract_port_from_bracket("clk"), "clk");
    }
}
