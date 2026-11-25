//! Segmented controls with a sliding selection.
//!
//! Every control where one of a few options is picked (the side panel's
//! Files / Changes / Search, quick open's Files / Contents, the diff's base
//! and layout, the settings' two-way choices) draws its selection as one
//! chip that moves under the segments on a spring when the choice changes,
//! instead of the old segment losing its fill and the new one gaining it.
//! That is a project rule (AGENTS.md): a new control of this kind uses
//! [`Slide::render`], not a hand-rolled selected background.

use super::*;
use gpui::{Animation, AnimationExt};
use std::{cell::RefCell, rc::Rc};

/// How long the spring runs. It has settled to within 1% by the end, so the
/// last frame's jump to exactly 1 is invisible.
const SPRING_DURATION: Duration = Duration::from_millis(420);

/// A damped spring from 0 to 1 over `SPRING_DURATION`: a quick start, a
/// slight overshoot (~6%), and a soft settle, the feel of a physical switch.
pub(super) fn spring(t: f32) -> f32 {
    if t >= 1. {
        return 1.;
    }
    if t <= 0. {
        return 0.;
    }
    // Natural frequency and damping ratio, in seconds of real time.
    const OMEGA: f32 = 18.;
    const ZETA: f32 = 0.66;
    let time = t * SPRING_DURATION.as_secs_f32();
    let damped = OMEGA * (1. - ZETA * ZETA).sqrt();
    let decay = (-ZETA * OMEGA * time).exp();
    1. - decay * ((damped * time).cos() + (ZETA * OMEGA / damped) * (damped * time).sin())
}

/// A segment's place in its control: left edge and width, relative to the
/// first segment.
type Span = (f32, f32);

#[derive(Default)]
struct SlideInner {
    /// The selection the chip is heading for.
    selected: Option<usize>,
    /// Where the chip set off from, for the transition in flight.
    from: Option<Span>,
    /// Bumped on every change so the animation restarts.
    generation: usize,
    /// The segments as laid out last frame.
    spans: Vec<Span>,
}

/// One segmented control's memory: where its segments are and where the
/// chip came from. Cheap to clone; keep one per control on its view.
#[derive(Clone, Default)]
pub(super) struct Slide(Rc<RefCell<SlideInner>>);

impl Slide {
    /// The control: `segments` in a row with the chip under the selected
    /// one. `chip` is the chip's look (fill, border, shadow, radius); the
    /// caller styles the track around the returned element. Segments must
    /// not paint a selected background of their own.
    pub(super) fn render(
        &self,
        id: &'static str,
        selected: usize,
        gap: Pixels,
        fill: bool,
        segments: Vec<AnyElement>,
        chip: Div,
    ) -> AnyElement {
        let (from, to, generation) = {
            let mut inner = self.0.borrow_mut();
            if inner.selected != Some(selected) {
                inner.from = inner
                    .selected
                    .and_then(|previous| inner.spans.get(previous).copied());
                inner.selected = Some(selected);
                inner.generation += 1;
            }
            (
                inner.from,
                inner.spans.get(selected).copied(),
                inner.generation,
            )
        };
        let indicator = match to {
            // Not laid out yet: the first frame measures, the next one draws.
            None => div().absolute().size_0().into_any_element(),
            Some((x, width)) => {
                let placed = chip.absolute().top_0().bottom_0().left(px(x)).w(px(width));
                match from.filter(|from| *from != (x, width)) {
                    None => placed.into_any_element(),
                    Some((from_x, from_width)) => placed
                        .with_animation(
                            ElementId::NamedInteger(id.into(), generation as u64),
                            Animation::new(SPRING_DURATION).with_easing(spring),
                            move |el, delta| {
                                el.left(px(from_x + (x - from_x) * delta))
                                    .w(px(from_width + (width - from_width) * delta))
                            },
                        )
                        .into_any_element(),
                }
            }
        };
        let memory = self.0.clone();
        div()
            .relative()
            .when(fill, |el| el.w_full())
            .flex()
            .items_center()
            .gap(gap)
            // The chip is the first child, so it paints under the segments.
            .child(indicator)
            .children(segments)
            .on_children_prepainted(move |bounds, window, _| {
                // Child 0 is the chip itself.
                let Some(first) = bounds.get(1) else {
                    return;
                };
                let origin = f32::from(first.origin.x);
                let spans: Vec<Span> = bounds[1..]
                    .iter()
                    .map(|b| (f32::from(b.origin.x) - origin, f32::from(b.size.width)))
                    .collect();
                let mut inner = memory.borrow_mut();
                if inner.spans != spans {
                    inner.spans = spans;
                    window.refresh();
                }
            })
            .into_any_element()
    }
}

/// The selection chip most controls use: raised paper with a hairline and
/// a 1px soft drop, as in the design.
pub(super) fn raised_chip(radius: f32, cx: &App) -> Div {
    let dark = cx.theme().is_dark();
    div()
        .rounded(px(radius))
        .border_1()
        .border_color(if dark {
            gpui::white().opacity(0.08)
        } else {
            gpui::black().opacity(0.07)
        })
        .bg(rgb(if dark { 0x2C2E32 } else { 0xFFFFFF }))
        .shadow(vec![BoxShadow {
            color: gpui::black().opacity(if dark { 0.35 } else { 0.08 }),
            offset: point(px(0.), px(1.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        }])
}

/// The recessed track a segmented control sits in.
pub(super) fn track(radius: f32, cx: &App) -> Div {
    div()
        .p(px(3.))
        .rounded(px(radius))
        .bg(if cx.theme().is_dark() {
            gpui::white().opacity(0.05)
        } else {
            gpui::black().opacity(0.05)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new selection sets the chip off from where the old one was laid
    /// out, and restarts the animation; re-rendering the same selection does
    /// neither.
    #[core::prelude::v1::test]
    fn a_new_selection_starts_from_the_old_segment() {
        let slide = Slide::default();
        let render = |selected| {
            let _ = slide.render("t", selected, px(0.), false, Vec::new(), div());
        };
        render(0);
        slide.0.borrow_mut().spans = vec![(0., 60.), (62., 80.), (144., 70.)];
        render(0);
        assert_eq!(slide.0.borrow().generation, 1);
        render(2);
        let inner = slide.0.borrow();
        assert_eq!(inner.generation, 2);
        assert_eq!(inner.from, Some((0., 60.)));
        assert_eq!(inner.selected, Some(2));
        drop(inner);
        render(2);
        assert_eq!(slide.0.borrow().generation, 2, "no change, no restart");
    }

    #[core::prelude::v1::test]
    fn the_spring_starts_at_rest_overshoots_a_little_and_lands() {
        assert_eq!(spring(0.), 0.);
        assert_eq!(spring(1.), 1.);
        let peak = (0..=100)
            .map(|i| spring(i as f32 / 100.))
            .fold(0f32, f32::max);
        assert!(peak > 1.02 && peak < 1.12, "overshoot {peak}");
        assert!((spring(0.95) - 1.).abs() < 0.02, "settled by the end");
        assert!(spring(0.1) > 0.15, "a quick start");
    }
}
