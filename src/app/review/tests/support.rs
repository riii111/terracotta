use super::super::{PlanBlock, PlanBlockKind, PlanDocument, PlanLineKind};

pub(crate) fn plan_document_with_blocks(text: String, blocks: Vec<PlanBlock>) -> PlanDocument {
    let line_kinds = vec![PlanLineKind::Body; text.split('\n').count()];
    PlanDocument::with_blocks_and_line_kinds(text, blocks, line_kinds)
}

pub(crate) fn plan_document(text: String) -> PlanDocument {
    let end = text.split('\n').count();
    plan_document_with_blocks(text, vec![PlanBlock::new(0..end, PlanBlockKind::Common)])
}
