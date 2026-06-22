//! Animated wrappers reused for page/region transitions.
//!
//! [`FadeIn`] animates its child from opacity 0→1 on mount.
//! [`SlideIn`] animates its child from `from_x` px offset → 0 on mount.
use freya::animation::*;
use freya::prelude::*;

/// Wraps a child element and fades it in (opacity 0 → 1) on mount.
#[derive(PartialEq, Clone)]
pub struct FadeIn {
    child: Element,
}

impl FadeIn {
    pub fn new(child: impl IntoElement) -> Self {
        Self { child: child.into_element() }
    }
}

impl Component for FadeIn {
    fn render(&self) -> impl IntoElement {
        let anim = use_animation(|conf| {
            conf.on_creation(OnCreation::Run);
            AnimNum::new(0.0, 1.0).time(200).ease(Ease::Out)
        });
        let opacity = anim.get().value();
        rect().opacity(opacity).child(self.child.clone())
    }
}

/// Wraps a child element and slides it in (offset_x `from_x` → 0) on mount.
#[derive(PartialEq, Clone)]
pub struct SlideIn {
    child: Element,
    from_x: f32,
}

impl SlideIn {
    pub fn new(child: impl IntoElement, from_x: f32) -> Self {
        Self { child: child.into_element(), from_x }
    }
}

impl Component for SlideIn {
    fn render(&self) -> impl IntoElement {
        let from = self.from_x;
        let anim = use_animation(move |conf| {
            conf.on_creation(OnCreation::Run);
            AnimNum::new(from, 0.0).time(220).ease(Ease::Out)
        });
        let dx = anim.get().value();
        rect().offset_x(dx).child(self.child.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn fade_in_renders_child() {
        fn app() -> impl IntoElement {
            FadeIn::new(label().text("INNER"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "INNER")
        });
        assert!(found.is_some(), "FadeIn should render its child label");
    }

    #[test]
    fn slide_in_renders_child() {
        fn app() -> impl IntoElement {
            SlideIn::new(label().text("SLIDE"), -30.0)
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "SLIDE")
        });
        assert!(found.is_some(), "SlideIn should render its child label");
    }
}
