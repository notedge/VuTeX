//! Implementation of the Knuth-Plass line-breaking algorithm.
//!
//! This algorithm finds the optimal sequence of line breaks by minimizing the "badness"
//! of each line, considering "boxes" (content), "glue" (flexible space), and "penalties" (break opportunities).

use serde::{Deserialize, Serialize};
use rutex_types::Fixed;

/// Represents an item in the Knuth-Plass model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Item {
    /// A fixed-width box that cannot be broken.
    Box {
        width: Fixed,
        /// Optional identifier or content reference.
        debug_info: Option<String>,
    },
    /// Flexible space between boxes.
    Glue {
        width: Fixed,
        stretch: Fixed,
        shrink: Fixed,
    },
    /// A potential break point with an associated penalty.
    Penalty {
        width: Fixed,
        penalty: f64,
        flagged: bool,
    },
}

impl Item {
    pub fn is_box(&self) -> bool {
        matches!(self, Item::Box { .. })
    }

    pub fn is_glue(&self) -> bool {
        matches!(self, Item::Glue { .. })
    }

    pub fn is_penalty(&self) -> bool {
        matches!(self, Item::Penalty { .. })
    }

    pub fn width(&self) -> Fixed {
        match self {
            Item::Box { width, .. } => *width,
            Item::Glue { width, .. } => *width,
            Item::Penalty { width, .. } => *width,
        }
    }

    pub fn penalty(&self) -> f64 {
        match self {
            Item::Penalty { penalty, .. } => *penalty,
            _ => 0.0,
        }
    }
}

/// A break point in the Knuth-Plass algorithm.
#[derive(Debug, Clone, Copy)]
pub struct BreakPoint {
    /// Index in the item list.
    pub index: usize,
    /// The active node from which this break was reached.
    pub previous: Option<usize>,
    /// Accumulated demerits up to this point.
    pub total_demerits: f64,
    /// Line number (0-indexed).
    pub line: usize,
    /// Fitness class of the line ending here.
    pub fitness_class: usize,
}

pub struct KnuthPlass {
    pub line_widths: Vec<Fixed>,
    pub tolerance: f64,
}

const INFINITY: f64 = 10000.0;
const LINE_PENALTY: f64 = 10.0; // Demerits for each line
const FLAGGED_PENALTY: f64 = 100.0; // Penalty for two consecutive flagged breaks

impl KnuthPlass {
    pub fn new(line_widths: Vec<Fixed>, tolerance: f64) -> Self {
        Self {
            line_widths,
            tolerance,
        }
    }

    /// Calculate badness of a line.
    /// ratio = (desired_width - actual_width) / total_stretch_or_shrink
    pub fn calculate_badness(ratio: f64) -> f64 {
        if ratio < -1.0 {
            return f64::INFINITY;
        }
        100.0 * ratio.abs().powi(3)
    }

    /// Determine fitness class based on ratio.
    pub fn fitness_class(ratio: f64) -> usize {
        if ratio < -0.5 {
            0 // Very tight
        } else if ratio <= 0.5 {
            1 // Tight
        } else if ratio <= 1.0 {
            2 // Loose
        } else {
            3 // Very loose
        }
    }

    /// Finds the optimal break points for a sequence of items.
    pub fn find_breaks(&self, items: &[Item]) -> Vec<usize> {
        // Add a virtual penalty at the end to force a break.
        let mut items = items.to_vec();
        if items.is_empty() || !matches!(items.last(), Some(Item::Penalty { penalty, .. }) if *penalty <= -INFINITY) {
            items.push(Item::Penalty { width: Fixed::ZERO, penalty: -INFINITY, flagged: false });
        }

        let mut all_nodes = vec![BreakPoint {
            index: 0,
            previous: None,
            total_demerits: 0.0,
            line: 0,
            fitness_class: 1,
        }];
        let mut active_node_indices = vec![0];

        let mut sum_width = vec![Fixed::ZERO; items.len() + 1];
        let mut sum_stretch = vec![Fixed::ZERO; items.len() + 1];
        let mut sum_shrink = vec![Fixed::ZERO; items.len() + 1];

        for (i, item) in items.iter().enumerate() {
            sum_width[i + 1] = sum_width[i] + item.width();
            match item {
                Item::Glue { stretch, shrink, .. } => {
                    sum_stretch[i + 1] = sum_stretch[i] + *stretch;
                    sum_shrink[i + 1] = sum_shrink[i] + *shrink;
                }
                _ => {
                    sum_stretch[i + 1] = sum_stretch[i];
                    sum_shrink[i + 1] = sum_shrink[i];
                }
            }
        }

        for (i, item) in items.iter().enumerate() {
            let is_potential_break = match item {
                Item::Penalty { penalty, .. } => *penalty < INFINITY,
                Item::Glue { .. } => i > 0 && items[i - 1].is_box(),
                _ => false,
            };

            if !is_potential_break {
                continue;
            }

            let mut best_for_class: [Option<BreakPoint>; 4] = [None; 4];
            let mut new_active_node_indices = Vec::new();

            for &node_idx in &active_node_indices {
                let node = &all_nodes[node_idx];
                let line_idx = node.line.min(self.line_widths.len() - 1);
                let target_width = self.line_widths[line_idx];

                // Calculate width, stretch and shrink for the line from node.index to i
                let mut actual_width = sum_width[i] - sum_width[node.index];
                if let Item::Penalty { width, .. } = item {
                    actual_width = actual_width + *width;
                }

                let available_stretch = sum_stretch[i] - sum_stretch[node.index];
                let available_shrink = sum_shrink[i] - sum_shrink[node.index];

                let diff = (target_width.0 - actual_width.0) as f64;
                let ratio = if diff > 0.0 {
                    if available_stretch.0 > 0 {
                        diff / available_stretch.0 as f64
                    } else {
                        INFINITY
                    }
                } else if diff < 0.0 {
                    if available_shrink.0 > 0 {
                        diff / available_shrink.0 as f64
                    } else {
                        -INFINITY
                    }
                } else {
                    0.0
                };

                if ratio < -1.0 || (item.is_penalty() && item.penalty() > -INFINITY && ratio > self.tolerance) {
                    continue;
                }

                let badness = if ratio.abs() > 100.0 { INFINITY } else { Self::calculate_badness(ratio) };
                let penalty = item.penalty();

                let mut demerits = (LINE_PENALTY + badness).powi(2);
                if penalty >= 0.0 {
                    demerits += penalty.powi(2);
                } else if penalty > -INFINITY {
                    demerits -= penalty.powi(2);
                }

                // Flagged penalty
                if let Item::Penalty { flagged: curr_flagged, .. } = item {
                    if *curr_flagged {
                        if let Some(prev_idx) = node.previous {
                            if let Item::Penalty { flagged: prev_flagged, .. } = items[all_nodes[prev_idx].index] {
                                if prev_flagged {
                                    demerits += FLAGGED_PENALTY.powi(2);
                                }
                            }
                        }
                    }
                }

                let fitness = Self::fitness_class(ratio);
                if (fitness as isize - node.fitness_class as isize).abs() > 1 {
                    demerits += 3000.0;
                }

                let total_demerits = node.total_demerits + demerits;
                
                if let Some(ref best) = best_for_class[fitness] {
                    if total_demerits < best.total_demerits {
                        best_for_class[fitness] = Some(BreakPoint {
                            index: i,
                            previous: Some(node_idx),
                            total_demerits,
                            line: node.line + 1,
                            fitness_class: fitness,
                        });
                    }
                } else {
                    best_for_class[fitness] = Some(BreakPoint {
                        index: i,
                        previous: Some(node_idx),
                        total_demerits,
                        line: node.line + 1,
                        fitness_class: fitness,
                    });
                }
            }

            for opt_node in best_for_class.iter() {
                if let Some(node) = opt_node {
                    all_nodes.push(*node);
                    new_active_node_indices.push(all_nodes.len() - 1);
                }
            }
            
            active_node_indices.extend(new_active_node_indices);
            
            if let Item::Penalty { penalty, .. } = item {
                if *penalty <= -INFINITY {
                    active_node_indices.retain(|&idx| all_nodes[idx].index == i);
                }
            }
        }

        let mut best_end_node_idx: Option<usize> = None;
        let mut min_demerits = f64::INFINITY;

        for &node_idx in &active_node_indices {
            let node = &all_nodes[node_idx];
            if node.total_demerits < min_demerits {
                min_demerits = node.total_demerits;
                best_end_node_idx = Some(node_idx);
            }
        }

        let mut breaks = Vec::new();
        let mut curr = best_end_node_idx;
        while let Some(idx) = curr {
            let node = &all_nodes[idx];
            if node.index > 0 && node.index < items.len() - 1 {
                breaks.push(node.index);
            }
            curr = node.previous;
        }
        breaks.reverse();
        breaks
    }
}
