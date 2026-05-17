use gpui::{AnyElement, IntoElement, ParentElement, linear_color_stop, linear_gradient};
use smallvec::SmallVec;
use ui::{Vector, VectorName, prelude::*};

#[derive(IntoElement)]
pub struct AgentPanelOnboardingCard {
    children: SmallVec<[AnyElement; 2]>,
}

impl AgentPanelOnboardingCard {
    pub fn new() -> Self {
        Self {
            children: SmallVec::new(),
        }
    }
}

impl ParentElement for AgentPanelOnboardingCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

// PaddleBoard: render gutted — was a decorative card wrapper around Zed
// AI plan content. Public surface preserved so call sites compile.
impl RenderOnce for AgentPanelOnboardingCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
    }
}
