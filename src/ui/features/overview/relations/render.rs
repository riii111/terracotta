use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::{
    app::plan::{
        RelationGraph, RelationGraphGroup, RelationGraphLink, RelationGraphLinkKind, RelationNode,
        RelationNodeId, RelationSource, RelationUnresolvedReason, ResourceChangeKind,
    },
    ui::theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelationGraphScroll {
    pub(crate) vertical: u16,
    pub(crate) horizontal: u16,
}

pub(crate) struct RelationGraphView<'a> {
    pub(crate) title: &'a str,
    pub(crate) selected_node: Option<&'a RelationNodeId>,
    pub(crate) focused: bool,
    pub(crate) maximized: bool,
    pub(crate) scroll: RelationGraphScroll,
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    graph: &RelationGraph,
    view: &RelationGraphView<'_>,
) -> RelationGraphScroll {
    let title = Line::from(vec![
        Span::styled(
            if view.focused { "* " } else { "  " },
            if view.focused {
                theme::relation_frame_style(true)
            } else {
                theme::relation_muted_style()
            },
        ),
        Span::styled(
            "[3] Relations",
            theme::relation_text_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" · {}", view.title), theme::relation_muted_style()),
    ]);
    let block = Block::bordered()
        .title(title)
        .border_style(theme::relation_frame_style(view.focused))
        .style(theme::relation_text_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return RelationGraphScroll {
            vertical: 0,
            horizontal: 0,
        };
    }

    let legend = vec![
        "A ──> B  B uses A",
        "solid=instance endpoints; dotted=block candidate",
        "state evidence=(state)",
    ]
    .into_iter()
    .map(|line| Line::from(Span::styled(line, theme::relation_muted_style())))
    .collect::<Vec<_>>();
    let legend_height = u16::try_from(legend.len())
        .unwrap_or(u16::MAX)
        .min(inner.height);
    let visible_legend = legend
        .into_iter()
        .take(usize::from(legend_height))
        .collect::<Vec<_>>();
    let content_height = inner.height.saturating_sub(legend_height);
    let content_area = Rect::new(inner.x, inner.y, inner.width, content_height);
    let legend_area = Rect::new(
        inner.x,
        inner.y.saturating_add(content_height),
        inner.width,
        legend_height,
    );
    let lines = graph_lines(
        graph,
        view.selected_node,
        view.maximized,
        content_area.width,
    );
    let max_vertical = offset_limit(lines.len(), usize::from(content_area.height));
    let max_horizontal = offset_limit(
        lines.iter().map(Line::width).max().unwrap_or_default(),
        usize::from(content_area.width),
    );
    let scroll = RelationGraphScroll {
        vertical: view.scroll.vertical.min(max_vertical),
        horizontal: view.scroll.horizontal.min(max_horizontal),
    };

    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::relation_text_style())
            .scroll((scroll.vertical, scroll.horizontal)),
        content_area,
    );
    frame.render_widget(Paragraph::new(visible_legend), legend_area);
    scroll
}

fn graph_lines(
    graph: &RelationGraph,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
    width: u16,
) -> Vec<Line<'static>> {
    if graph.nodes.is_empty() {
        return vec![Line::from(Span::styled(
            "No changes to show",
            theme::relation_muted_style(),
        ))];
    }

    let node_index = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    let mut links_by_node = BTreeMap::<RelationNodeId, Vec<&RelationGraphLink>>::new();
    for link in &graph.links {
        links_by_node
            .entry(link.from.clone())
            .or_default()
            .push(link);
        links_by_node.entry(link.to.clone()).or_default().push(link);
    }

    let mut output_lines = Vec::new();
    for group in &graph.connected_groups {
        let ids = group.nodes.iter().cloned().collect::<BTreeSet<_>>();
        let mut group_links = BTreeMap::new();
        for id in &group.nodes {
            for link in links_by_node.get(id).into_iter().flatten() {
                if ids.contains(&link.from) && ids.contains(&link.to) {
                    group_links
                        .entry((link.from.clone(), link.to.clone()))
                        .or_insert(*link);
                }
            }
        }
        let links = group_links.into_values().collect::<Vec<_>>();
        let diagram = diagram_lines(group, &links, &node_index, selected_node, maximized);
        if let Some(diagram) = diagram
            && diagram
                .iter()
                .all(|line| line.width() <= usize::from(width))
        {
            push_section_lines(&mut output_lines, diagram);
        } else {
            let fallback =
                fallback_group_lines(group, &links, &node_index, selected_node, maximized);
            push_section_lines(&mut output_lines, fallback);
        }
    }

    let unknown = graph
        .links_unknown
        .iter()
        .filter_map(|id| node(&node_index, id))
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        push_section_heading(&mut output_lines, "Links unknown");
        for node in unknown {
            let mut row = node_line(node, selected_node, maximized);
            row.spans.push(Span::raw("  "));
            row.spans.push(Span::styled(
                node.unresolved
                    .iter()
                    .copied()
                    .map(unresolved_label)
                    .collect::<Vec<_>>()
                    .join(", "),
                theme::relation_text_style(),
            ));
            output_lines.push(row);
        }
    }

    let no_links = graph
        .no_links_shown
        .iter()
        .filter_map(|id| node(&node_index, id))
        .collect::<Vec<_>>();
    if !no_links.is_empty() {
        push_section_heading(&mut output_lines, "No links shown");
        output_lines.extend(
            no_links
                .into_iter()
                .map(|node| node_line(node, selected_node, maximized)),
        );
    }

    output_lines
}

fn diagram_lines(
    group: &RelationGraphGroup,
    links: &[&RelationGraphLink],
    node_index: &BTreeMap<RelationNodeId, &RelationNode>,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
) -> Option<Vec<Line<'static>>> {
    match (group.nodes.len(), links.len()) {
        (2, 1) => {
            let link = links[0];
            Some(vec![link_line(link, node_index, selected_node, maximized)])
        }
        (3, 2) => {
            if let Some((source, dependents)) = branch_links(links) {
                branch_lines(source, dependents, node_index, selected_node, maximized)
            } else if let Some((sources, target)) = merge_links(links) {
                merge_diagram_lines(sources, target, node_index, selected_node, maximized)
            } else if let Some(ordered) = chain_links(links) {
                let mut spans = node_line(
                    node(node_index, &ordered[0].from)?,
                    selected_node,
                    maximized,
                )
                .spans;
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    edge_segment(ordered[0]),
                    theme::relation_muted_style(),
                ));
                spans.extend(
                    node_line(node(node_index, &ordered[0].to)?, selected_node, maximized).spans,
                );
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    edge_segment(ordered[1]),
                    theme::relation_muted_style(),
                ));
                spans.extend(
                    node_line(node(node_index, &ordered[1].to)?, selected_node, maximized).spans,
                );
                Some(vec![Line::from(spans)])
            } else {
                None
            }
        }
        _ => None,
    }
}

fn branch_lines(
    source: &RelationNodeId,
    dependents: [&RelationGraphLink; 2],
    node_index: &BTreeMap<RelationNodeId, &RelationNode>,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
) -> Option<Vec<Line<'static>>> {
    let source_line = node_line(node(node_index, source)?, selected_node, maximized);
    let source_width = source_line.width();
    let edge_prefixes = dependents.map(edge_prefix);
    let max_prefix_width = edge_prefixes
        .iter()
        .map(|prefix| text_width(prefix))
        .max()
        .unwrap_or_default();
    let mut first = source_line.spans;
    first.push(Span::raw(" "));
    first.push(Span::styled(
        edge_prefixes[0].clone(),
        theme::relation_muted_style(),
    ));
    first.push(Span::raw(
        " ".repeat(max_prefix_width - text_width(&edge_prefixes[0])),
    ));
    first.push(Span::styled("┬─>", theme::relation_muted_style()));
    first.extend(
        node_line(
            node(node_index, &dependents[0].to)?,
            selected_node,
            maximized,
        )
        .spans,
    );

    let mut second = vec![Span::raw(" ".repeat(source_width + 1 + max_prefix_width))];
    second.push(Span::styled("└", theme::relation_muted_style()));
    second.push(Span::styled(
        edge_segment(dependents[1]),
        theme::relation_muted_style(),
    ));
    second.extend(
        node_line(
            node(node_index, &dependents[1].to)?,
            selected_node,
            maximized,
        )
        .spans,
    );
    Some(vec![Line::from(first), Line::from(second)])
}

fn merge_diagram_lines(
    sources: [&RelationGraphLink; 2],
    target: &RelationNodeId,
    node_index: &BTreeMap<RelationNodeId, &RelationNode>,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
) -> Option<Vec<Line<'static>>> {
    let source_lines = sources
        .iter()
        .map(|link| {
            node(node_index, &link.from).map(|node| node_line(node, selected_node, maximized))
        })
        .collect::<Option<Vec<_>>>()?;
    let aligned_width = source_lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or_default();
    let edge_prefixes = sources.map(edge_prefix);
    let max_prefix_width = edge_prefixes
        .iter()
        .map(|prefix| text_width(prefix))
        .max()
        .unwrap_or_default();

    let mut first = source_lines[0].spans.clone();
    first.push(Span::raw(
        " ".repeat(aligned_width - source_lines[0].width() + 1),
    ));
    first.push(Span::styled(
        edge_prefixes[0].clone(),
        theme::relation_muted_style(),
    ));
    first.push(Span::raw(
        " ".repeat(max_prefix_width - text_width(&edge_prefixes[0])),
    ));
    first.push(Span::styled("┐", theme::relation_muted_style()));

    let mut second = source_lines[1].spans.clone();
    second.push(Span::raw(
        " ".repeat(aligned_width - source_lines[1].width() + 1),
    ));
    second.push(Span::styled(
        edge_prefixes[1].clone(),
        theme::relation_muted_style(),
    ));
    second.push(Span::raw(
        " ".repeat(max_prefix_width - text_width(&edge_prefixes[1])),
    ));
    second.push(Span::styled("┴─>", theme::relation_muted_style()));
    second.extend(node_line(node(node_index, target)?, selected_node, maximized).spans);
    Some(vec![Line::from(first), Line::from(second)])
}

fn branch_links<'a>(
    links: &[&'a RelationGraphLink],
) -> Option<(&'a RelationNodeId, [&'a RelationGraphLink; 2])> {
    let source = &links.first()?.from;
    if links.iter().all(|link| &link.from == source) && links[0].to != links[1].to {
        let mut dependents = [links[0], links[1]];
        dependents.sort_by(|left, right| left.to.cmp(&right.to));
        Some((source, dependents))
    } else {
        None
    }
}

fn merge_links<'a>(
    links: &[&'a RelationGraphLink],
) -> Option<([&'a RelationGraphLink; 2], &'a RelationNodeId)> {
    let target = &links.first()?.to;
    if links.iter().all(|link| &link.to == target) && links[0].from != links[1].from {
        let mut sources = [links[0], links[1]];
        sources.sort_by(|left, right| left.from.cmp(&right.from));
        Some((sources, target))
    } else {
        None
    }
}

fn chain_links<'a>(links: &[&'a RelationGraphLink]) -> Option<[&'a RelationGraphLink; 2]> {
    let (first, second) = if links[0].to == links[1].from {
        (links[0], links[1])
    } else if links[1].to == links[0].from {
        (links[1], links[0])
    } else {
        return None;
    };
    (first.from != second.to).then_some([first, second])
}

fn link_line(
    link: &RelationGraphLink,
    node_index: &BTreeMap<RelationNodeId, &RelationNode>,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
) -> Line<'static> {
    let mut spans = node_line(
        node(node_index, &link.from).expect("graph link source exists"),
        selected_node,
        maximized,
    )
    .spans;
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        edge_segment(link),
        theme::relation_muted_style(),
    ));
    spans.extend(
        node_line(
            node(node_index, &link.to).expect("graph link target exists"),
            selected_node,
            maximized,
        )
        .spans,
    );
    Line::from(spans)
}

fn fallback_group_lines(
    group: &RelationGraphGroup,
    links: &[&RelationGraphLink],
    node_index: &BTreeMap<RelationNodeId, &RelationNode>,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
) -> Vec<Line<'static>> {
    let mut incoming = BTreeMap::<RelationNodeId, Vec<&RelationGraphLink>>::new();
    for link in links {
        incoming.entry(link.to.clone()).or_default().push(link);
    }
    group
        .nodes
        .iter()
        .filter_map(|id| {
            let mut row = node_line(node(node_index, id)?, selected_node, maximized);
            let uses = incoming
                .get(id)
                .into_iter()
                .flatten()
                .map(|link| {
                    format!(
                        "{} ({})",
                        node(node_index, &link.from)
                            .map_or_else(|| link.from.addresses().join(", "), node_path,),
                        evidence_label(link)
                    )
                })
                .collect::<Vec<_>>();
            if !uses.is_empty() {
                row.spans.push(Span::styled(
                    format!("  uses: {}", uses.join(", ")),
                    theme::relation_muted_style(),
                ));
            }
            Some(row)
        })
        .collect()
}

fn node_line(
    node: &RelationNode,
    selected_node: Option<&RelationNodeId>,
    maximized: bool,
) -> Line<'static> {
    let selected = selected_node == Some(&node.id);
    let mut spans = vec![
        Span::styled(
            if selected { "> " } else { "  " },
            theme::relation_text_style(),
        ),
        Span::styled(
            operation_symbol(node.operation),
            operation_style(node.operation),
        ),
        Span::styled(" ", theme::relation_text_style()),
    ];
    let breadcrumbs = visible_breadcrumbs(&node.breadcrumbs, maximized);
    if !breadcrumbs.is_empty() {
        spans.push(Span::styled(
            breadcrumbs.join(" › "),
            theme::relation_muted_style(),
        ));
        spans.push(Span::styled(" › ", theme::relation_muted_style()));
    }
    let mut address_style = theme::relation_text_style();
    if selected {
        address_style = address_style.add_modifier(Modifier::UNDERLINED);
    }
    if node.differs {
        address_style = address_style.add_modifier(Modifier::BOLD);
    }
    spans.push(Span::styled(node.display_address.clone(), address_style));
    if node.display_address.contains("[*]") && node.change_count > 0 {
        spans.push(Span::styled(
            format!(" ×{}", node.change_count),
            theme::relation_muted_style(),
        ));
    }
    if node.differs {
        spans.push(Span::styled(" !", theme::relation_warning_style()));
    }
    if !node.unresolved.is_empty() {
        spans.push(Span::styled(" ?", theme::relation_muted_style()));
    }
    Line::from(spans)
}

fn visible_breadcrumbs(breadcrumbs: &[String], maximized: bool) -> Vec<String> {
    if maximized || breadcrumbs.len() < 3 {
        return breadcrumbs.to_vec();
    }
    vec![
        breadcrumbs[0].clone(),
        "…".to_owned(),
        breadcrumbs[breadcrumbs.len() - 1].clone(),
    ]
}

fn node_path(node: &RelationNode) -> String {
    let mut parts = node.breadcrumbs.clone();
    parts.push(node.display_address.clone());
    parts.join(" › ")
}

fn edge_prefix(link: &RelationGraphLink) -> String {
    edge_segment(link).trim_end_matches('>').to_owned()
}

fn text_width(text: &str) -> usize {
    Line::from(Span::raw(text.to_owned())).width()
}

fn node<'a>(
    index: &BTreeMap<RelationNodeId, &'a RelationNode>,
    id: &RelationNodeId,
) -> Option<&'a RelationNode> {
    index.get(id).copied()
}

const fn operation_symbol(operation: ResourceChangeKind) -> &'static str {
    match operation {
        ResourceChangeKind::Create | ResourceChangeKind::Import => "+",
        ResourceChangeKind::Update => "~",
        ResourceChangeKind::Replace => "-/+",
        ResourceChangeKind::Delete => "-",
        ResourceChangeKind::NoOp => "=",
        ResourceChangeKind::Read => "r",
        ResourceChangeKind::Move => "m",
        ResourceChangeKind::Unknown => "?",
        ResourceChangeKind::Unsupported => "!",
    }
}

fn operation_style(operation: ResourceChangeKind) -> ratatui::style::Style {
    match operation {
        ResourceChangeKind::Create | ResourceChangeKind::Import => theme::relation_create_style(),
        ResourceChangeKind::Update => theme::relation_update_style(),
        ResourceChangeKind::Replace => theme::relation_replace_style(),
        ResourceChangeKind::Delete => theme::relation_delete_style(),
        ResourceChangeKind::Unsupported => theme::relation_warning_style(),
        ResourceChangeKind::NoOp
        | ResourceChangeKind::Read
        | ResourceChangeKind::Move
        | ResourceChangeKind::Unknown => theme::relation_text_style(),
    }
}

fn edge_segment(link: &RelationGraphLink) -> String {
    let glyph = match link.kind {
        RelationGraphLinkKind::Solid => "─",
        RelationGraphLinkKind::Dotted => "┄",
    };
    let evidence = source_label(link);
    if evidence.is_empty() {
        format!("{glyph}{glyph}>")
    } else {
        format!("{glyph}({evidence}){glyph}>")
    }
}

fn evidence_label(link: &RelationGraphLink) -> String {
    let mut labels = source_label(link);
    if link.kind == RelationGraphLinkKind::Dotted {
        if !labels.is_empty() {
            labels.push_str(", ");
        }
        labels.push_str("block candidate");
    }
    labels
}

fn source_label(link: &RelationGraphLink) -> String {
    link.sources
        .iter()
        .map(|source| match source {
            RelationSource::Configuration => "config",
            RelationSource::State => "state",
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn push_section_heading(lines: &mut Vec<Line<'static>>, heading: &'static str) {
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled(
        heading,
        theme::relation_section_style(),
    )));
}

fn push_section_lines(lines: &mut Vec<Line<'static>>, section: Vec<Line<'static>>) {
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.extend(section);
}

const fn unresolved_label(reason: RelationUnresolvedReason) -> &'static str {
    match reason {
        RelationUnresolvedReason::LocalValue => "local unresolved",
        RelationUnresolvedReason::Variable => "module variable unresolved",
        RelationUnresolvedReason::MissingAddress => "address unresolved",
        RelationUnresolvedReason::AmbiguousModule => "module reference unresolved",
        RelationUnresolvedReason::CyclicReference => "reference cycle",
        RelationUnresolvedReason::InvalidConfiguration => "invalid configuration relation",
        RelationUnresolvedReason::InvalidState => "invalid state relation",
        RelationUnresolvedReason::ConfigurationPartial => "configuration partial",
        RelationUnresolvedReason::ConfigurationUnavailable => "configuration unavailable",
        RelationUnresolvedReason::ConfigurationNotCollected => "configuration not collected",
        RelationUnresolvedReason::StateUnavailable => "state unavailable",
        RelationUnresolvedReason::StateNotCollected => "state not collected",
    }
}

fn offset_limit(content: usize, viewport: usize) -> u16 {
    u16::try_from(content.saturating_sub(viewport)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ratatui::{layout::Rect, style::Color};

    use crate::{
        app::plan::{
            RelationGraph, RelationGraphGroup, RelationGraphLink, RelationGraphLinkKind,
            RelationNode, RelationNodeId, RelationSource, RelationUnresolvedReason,
            ResourceChangeKind,
        },
        ui::{
            test_support::{buffer_text, render_to_buffer},
            theme,
        },
    };

    use super::{RelationGraphScroll, RelationGraphView, render};

    #[test]
    fn branch_keeps_all_nodes_and_evidence_visible() {
        let graph = branch_graph();
        let output = render_to_buffer((165, 50), |frame| {
            render(
                frame,
                Rect::new(0, 0, 165, 50),
                &graph,
                &RelationGraphView {
                    title: "prod · whole env",
                    selected_node: None,
                    focused: true,
                    maximized: false,
                    scroll: RelationGraphScroll {
                        vertical: 0,
                        horizontal: 0,
                    },
                },
            );
        });
        let text = buffer_text(&output);

        assert!(text.contains("terraform_data.db"), "{text}");
        assert!(text.contains("aws_ecs_service.api"));
        assert!(text.contains("aws_route53_record.db"));
        assert!(text.contains("┬─>"));
        assert!(text.contains("(state)"));
        assert!(text.contains("(config,state)"));
        assert!(text.contains("A ──> B  B uses A"));
        assert!(text.contains("solid=instance endpoints; dotted=block candidate"));

        let rows = text.lines().collect::<Vec<_>>();
        let top = rows.iter().position(|row| row.contains('┬')).unwrap();
        assert_eq!(
            glyph_column(rows[top], '┬'),
            glyph_column(rows[top + 1], '└')
        );
    }

    #[test]
    fn chains_and_merges_render_with_direction_and_source_annotations() {
        let chain = chain_graph();
        let chain_text = buffer_text(&render_to_buffer((165, 50), |frame| {
            render(
                frame,
                Rect::new(0, 0, 165, 50),
                &chain,
                &view("prod · whole env", None, false, 0, 0),
            );
        }));
        let merge = merge_graph();
        let merge_text = buffer_text(&render_to_buffer((165, 50), |frame| {
            render(
                frame,
                Rect::new(0, 0, 165, 50),
                &merge,
                &view("prod · whole env", None, false, 0, 0),
            );
        }));

        assert!(
            chain_text.contains("aws_vpc.main ─(config)─>"),
            "{chain_text}"
        );
        assert!(chain_text.contains("aws_subnet.web ─(state)─>"));
        assert!(merge_text.contains("┴─>"));
        assert!(merge_text.contains("(config,state)"));
        assert!(merge_text.contains("┐"), "{merge_text}");

        let merge_rows = merge_text.lines().collect::<Vec<_>>();
        let top = merge_rows
            .iter()
            .position(|row| row.contains('┐') && row.contains("aws_"))
            .unwrap();
        assert_eq!(
            glyph_column(merge_rows[top], '┐'),
            glyph_column(merge_rows[top + 1], '┴')
        );
    }

    #[test]
    fn unsupported_cycles_fall_back_to_incoming_uses_rows() {
        let graph = cycle_graph();
        let text = buffer_text(&render_to_buffer((120, 40), |frame| {
            render(
                frame,
                Rect::new(0, 0, 120, 40),
                &graph,
                &view("dev · whole env", None, true, 0, 0),
            );
        }));

        assert!(text.contains("uses: aws_db.main (config)"), "{text}");
        assert!(text.contains("uses: aws_service.api (state, block candidate)"));
        assert!(!text.contains("aws_db.main ──>"));
        assert!(!text.contains("aws_service.api ──>"));
    }

    #[test]
    fn unknown_reasons_precede_no_links_and_remain_distinct() {
        let graph = isolated_graph();
        let text = buffer_text(&render_to_buffer((80, 24), |frame| {
            render(
                frame,
                Rect::new(0, 0, 80, 24),
                &graph,
                &view("whole env", None, false, 0, 0),
            );
        }));

        assert!(text.find("Links unknown").unwrap() < text.find("No links shown").unwrap());
        assert!(text.contains("configuration unavailable"));
        assert!(text.contains("state unavailable"));
        assert!(text.contains("module reference unresolved"));
        assert!(text.contains("terraform_data.db"));
        assert!(text.contains("aws_security_group.web"));
        let unresolved_node = text
            .lines()
            .find(|line| line.contains("terraform_data.db"))
            .unwrap();
        assert_eq!(unresolved_node.matches('?').count(), 1, "{unresolved_node}");
    }

    #[test]
    fn fallback_uses_full_module_path_to_distinguish_references() {
        let x = node_with(
            "module.a.module.x.module.z.aws_db.main",
            "aws_db.main",
            ResourceChangeKind::Update,
            1,
            &["a", "x", "z"],
            false,
            &[],
        );
        let y = node_with(
            "module.a.module.y.module.z.aws_db.main",
            "aws_db.main",
            ResourceChangeKind::Update,
            1,
            &["a", "y", "z"],
            false,
            &[],
        );
        let third = node("aws_vpc.main", ResourceChangeKind::Update);
        let dependent = node("aws_service.api", ResourceChangeKind::Update);
        let graph = graph(
            vec![x.clone(), y.clone(), third.clone(), dependent.clone()],
            vec![
                link(
                    &x,
                    &dependent,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration],
                ),
                link(
                    &y,
                    &dependent,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration],
                ),
                link(
                    &third,
                    &dependent,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration],
                ),
            ],
        );
        let text = buffer_text(&render_to_buffer((165, 50), |frame| {
            render(
                frame,
                Rect::new(0, 0, 165, 50),
                &graph,
                &view("whole env", None, false, 0, 0),
            );
        }));

        assert!(text.contains("a › x › z › aws_db.main (config)"), "{text}");
        assert!(text.contains("a › y › z › aws_db.main (config)"), "{text}");
        assert!(!text.contains("a › … › z › aws_db.main (config)"));
    }

    #[test]
    fn selected_diff_node_keeps_operation_color_and_underlines_only_address() {
        let graph = single_node_graph();
        let output = render_to_buffer((80, 24), |frame| {
            render(
                frame,
                Rect::new(0, 0, 80, 24),
                &graph,
                &view(
                    "prod · not compared",
                    graph.nodes.first().map(|node| &node.id),
                    false,
                    0,
                    0,
                ),
            );
        });

        let text = buffer_text(&output);
        assert!(
            text.contains("> -/+ app › … › dns › aws_db.main[*] ×3 !"),
            "{text}"
        );
        assert_eq!(output.cell((1, 2)).unwrap().symbol(), ">");
        assert_eq!(output.cell((3, 2)).unwrap().fg, Color::Magenta);
        assert!(
            !output
                .cell((3, 2))
                .unwrap()
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );
        assert!(
            output
                .cell((23, 2))
                .unwrap()
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );
        assert!(
            output
                .cell((23, 2))
                .unwrap()
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(output.cell((0, 0)).unwrap().fg, Color::DarkGray);
        assert_eq!(output.cell((1, 23)).unwrap().fg, Color::DarkGray);

        let expanded_text = buffer_text(&render_to_buffer((100, 30), |frame| {
            render(
                frame,
                Rect::new(0, 0, 100, 30),
                &graph,
                &RelationGraphView {
                    title: "prod · whole env",
                    selected_node: None,
                    focused: false,
                    maximized: true,
                    scroll: RelationGraphScroll {
                        vertical: 0,
                        horizontal: 0,
                    },
                },
            );
        }));
        assert!(expanded_text.contains("app › net › dns › aws_db.main[*]"));
        assert!(!expanded_text.contains("app › … › dns"));
    }

    #[test]
    fn narrow_view_keeps_frame_and_bottom_legend_and_clamps_scroll() {
        let graph = long_graph();
        let mut returned = RelationGraphScroll {
            vertical: 0,
            horizontal: 0,
        };
        let output = render_to_buffer((40, 16), |frame| {
            returned = render(
                frame,
                Rect::new(0, 0, 40, 16),
                &graph,
                &view("prod · whole env", None, false, u16::MAX, u16::MAX),
            );
        });
        let text = buffer_text(&output);

        assert!(returned.vertical > 0);
        assert!(returned.horizontal > 0);
        assert!(text.contains("┌"));
        assert!(text.contains("this_19"));
        assert!(
            text.lines()
                .rev()
                .nth(1)
                .unwrap()
                .contains("state evidence=(state)")
        );
    }

    #[test]
    fn focused_and_operation_styles_use_terminal_palette_colors() {
        assert_eq!(theme::relation_frame_style(true).fg, Some(Color::Cyan));
        assert_eq!(theme::relation_create_style().fg, Some(Color::Green));
        assert_eq!(theme::relation_update_style().fg, Some(Color::Yellow));
        assert_eq!(theme::relation_replace_style().fg, Some(Color::Magenta));
        assert_eq!(theme::relation_delete_style().fg, Some(Color::Red));
        assert_eq!(theme::relation_text_style().bg, Some(Color::Reset));
    }

    fn view<'a>(
        title: &'a str,
        selected_node: Option<&'a RelationNodeId>,
        focused: bool,
        vertical: u16,
        horizontal: u16,
    ) -> RelationGraphView<'a> {
        RelationGraphView {
            title,
            selected_node,
            focused,
            maximized: false,
            scroll: RelationGraphScroll {
                vertical,
                horizontal,
            },
        }
    }

    fn glyph_column(line: &str, glyph: char) -> usize {
        line.chars()
            .position(|character| character == glyph)
            .unwrap()
    }

    fn node(address: &str, operation: ResourceChangeKind) -> RelationNode {
        let parts = address.split('.').collect::<Vec<_>>();
        let resource_index = parts
            .iter()
            .position(|part| part.starts_with("aws_") || part.starts_with("terraform_"))
            .unwrap_or_default();
        let breadcrumbs = parts[..resource_index]
            .chunks(2)
            .filter_map(|pair| {
                (pair.first() == Some(&"module"))
                    .then(|| pair.get(1).copied())
                    .flatten()
            })
            .map(str::to_owned)
            .collect();
        RelationNode {
            id: RelationNodeId::from_addresses([address.to_owned()]).unwrap(),
            display_address: parts[resource_index..].join("."),
            operation,
            change_count: 1,
            breadcrumbs,
            differs: false,
            unresolved: BTreeSet::new(),
        }
    }

    fn node_with(
        address: &str,
        display_address: &str,
        operation: ResourceChangeKind,
        change_count: usize,
        breadcrumbs: &[&str],
        differs: bool,
        unresolved: &[RelationUnresolvedReason],
    ) -> RelationNode {
        RelationNode {
            id: RelationNodeId::from_addresses([address.to_owned()]).unwrap(),
            display_address: display_address.to_owned(),
            operation,
            change_count,
            breadcrumbs: breadcrumbs.iter().map(|part| (*part).to_owned()).collect(),
            differs,
            unresolved: unresolved.iter().copied().collect(),
        }
    }

    fn link(
        from: &RelationNode,
        to: &RelationNode,
        kind: RelationGraphLinkKind,
        sources: &[RelationSource],
    ) -> RelationGraphLink {
        RelationGraphLink {
            from: from.id.clone(),
            to: to.id.clone(),
            kind,
            sources: sources.iter().copied().collect(),
        }
    }

    fn graph(nodes: Vec<RelationNode>, links: Vec<RelationGraphLink>) -> RelationGraph {
        let linked = links
            .iter()
            .flat_map(|link| [link.from.clone(), link.to.clone()])
            .collect::<BTreeSet<_>>();
        let group_nodes = linked.iter().cloned().collect::<Vec<_>>();
        let connected_groups = if group_nodes.is_empty() {
            Vec::new()
        } else {
            vec![RelationGraphGroup {
                nodes: group_nodes,
                contains_destructive_change: false,
            }]
        };
        let links_unknown = nodes
            .iter()
            .filter(|node| !linked.contains(&node.id) && !node.unresolved.is_empty())
            .map(|node| node.id.clone())
            .collect();
        let no_links_shown = nodes
            .iter()
            .filter(|node| !linked.contains(&node.id) && node.unresolved.is_empty())
            .map(|node| node.id.clone())
            .collect();
        RelationGraph {
            nodes,
            links,
            connected_groups,
            links_unknown,
            no_links_shown,
        }
    }

    fn branch_graph() -> RelationGraph {
        let source = node("module.app.terraform_data.db", ResourceChangeKind::Replace);
        let service = node("module.app.aws_ecs_service.api", ResourceChangeKind::Update);
        let record = node(
            "module.app.module.dns.aws_route53_record.db",
            ResourceChangeKind::Update,
        );
        graph(
            vec![source.clone(), service.clone(), record.clone()],
            vec![
                link(
                    &source,
                    &service,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration, RelationSource::State],
                ),
                link(
                    &source,
                    &record,
                    RelationGraphLinkKind::Dotted,
                    &[RelationSource::State],
                ),
            ],
        )
    }

    fn cycle_graph() -> RelationGraph {
        let database = node("aws_db.main", ResourceChangeKind::Update);
        let service = node("aws_service.api", ResourceChangeKind::Replace);
        graph(
            vec![database.clone(), service.clone()],
            vec![
                link(
                    &database,
                    &service,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration],
                ),
                link(
                    &service,
                    &database,
                    RelationGraphLinkKind::Dotted,
                    &[RelationSource::State],
                ),
            ],
        )
    }

    fn chain_graph() -> RelationGraph {
        let vpc = node("aws_vpc.main", ResourceChangeKind::Create);
        let subnet = node("aws_subnet.web", ResourceChangeKind::Update);
        let instance = node("aws_instance.api", ResourceChangeKind::Create);
        graph(
            vec![vpc.clone(), subnet.clone(), instance.clone()],
            vec![
                link(
                    &vpc,
                    &subnet,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration],
                ),
                link(
                    &subnet,
                    &instance,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::State],
                ),
            ],
        )
    }

    fn merge_graph() -> RelationGraph {
        let config = node("aws_vpc.main", ResourceChangeKind::Update);
        let state = node("aws_subnet.web", ResourceChangeKind::Update);
        let dependent = node("aws_instance.api", ResourceChangeKind::Replace);
        graph(
            vec![config.clone(), state.clone(), dependent.clone()],
            vec![
                link(
                    &config,
                    &dependent,
                    RelationGraphLinkKind::Solid,
                    &[RelationSource::Configuration, RelationSource::State],
                ),
                link(
                    &state,
                    &dependent,
                    RelationGraphLinkKind::Dotted,
                    &[RelationSource::Configuration],
                ),
            ],
        )
    }

    fn isolated_graph() -> RelationGraph {
        graph(
            vec![
                node_with(
                    "module.app.terraform_data.db",
                    "terraform_data.db",
                    ResourceChangeKind::Delete,
                    1,
                    &["app"],
                    false,
                    &[RelationUnresolvedReason::ConfigurationUnavailable],
                ),
                node_with(
                    "module.app.aws_security_group.web",
                    "aws_security_group.web",
                    ResourceChangeKind::Create,
                    1,
                    &["app"],
                    false,
                    &[RelationUnresolvedReason::AmbiguousModule],
                ),
                node_with(
                    "module.app.aws_route53_record.db",
                    "aws_route53_record.db",
                    ResourceChangeKind::Update,
                    1,
                    &["app"],
                    false,
                    &[RelationUnresolvedReason::StateUnavailable],
                ),
                node("aws_vpc.main", ResourceChangeKind::Update),
            ],
            Vec::new(),
        )
    }

    fn single_node_graph() -> RelationGraph {
        graph(
            vec![node_with(
                "module.app.module.dns.aws_db.main[0]",
                "aws_db.main[*]",
                ResourceChangeKind::Replace,
                3,
                &["app", "net", "dns"],
                true,
                &[],
            )],
            Vec::new(),
        )
    }

    fn long_graph() -> RelationGraph {
        graph(
            (0..20)
                .map(|index| {
                    node_with(
                        &format!("aws_long_resource.this_{index}"),
                        &format!("aws_long_resource_with_many_segments.this_{index}"),
                        ResourceChangeKind::Update,
                        1,
                        &["a_very_long_module_name", "nested_module"],
                        false,
                        &[],
                    )
                })
                .collect(),
            Vec::new(),
        )
    }
}
