use rune::{ScrollPos, Spring, Tx, WheelEvent};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Motion {
    target: Option<f32>,
    direction: f32,
    last_notch: Option<Instant>,
}

impl Motion {
    pub fn reset(&mut self) {
        self.target = None;
        self.last_notch = None;
    }

    pub fn wheel(
        &mut self,
        viewport: &str,
        at: ScrollPos,
        event: WheelEvent,
        tx: &mut Tx,
        reduced: bool,
    ) -> bool {
        if event.mods.shift
            || event.mods.ctrl
            || event.mods.cmd
            || event.mods.alt
            || event.mods.logo
            || event.dx.abs() > event.dy.abs()
        {
            self.reset();
            return false;
        }
        if !event.dy.is_finite() || event.dy == 0. || at.max[1] <= 0. {
            return false;
        }
        if reduced || event.precise || event.momentum {
            self.reset();
            // A captured gesture must apply precise deltas itself rather than discard them.
            tx.scroll_to_instant(viewport, 0., (at.offset[1] - event.dy).clamp(0., at.max[1]));
            return true;
        }
        let direction = event.dy.signum();
        let continuing = self.direction == direction
            && self
                .last_notch
                .is_some_and(|time| time.elapsed() < Duration::from_millis(120));
        let start = if continuing {
            self.target.unwrap_or(at.offset[1])
        } else {
            at.offset[1]
        };
        let target = (start - event.dy).clamp(0., at.max[1]);
        self.target = Some(target);
        self.direction = direction;
        self.last_notch = Some(Instant::now());
        // Rune retains velocity when repeated notches retarget the spring.
        tx.scroll_to_with(viewport, 0., target, Spring::new(0.16, 0.));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NoteMode, Release, State};
    use rune::{Env, Runtime};

    fn runtime(mode: NoteMode, reduced: bool) -> Runtime<State> {
        let mut state = State::demo();
        state.select_package("gh");
        state.mode = mode;
        let p = state
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == "gh")
            .unwrap();
        p.releases = (0..3).map(|i| Release {
            version: format!("{}.0.0", 3 - i),
            url: format!("https://example.com/release/{i}"),
            published: "2026-11-14".into(),
            markdown: "## Improvements\n\nFixed configuration loading and improved error reporting.\n\n".repeat(12),
        }).collect();
        let view = if reduced {
            |s: &State, cx: &mut rune::Cx<State>| {
                cx.reduced_motion(true);
                crate::ui::view(s, cx)
            }
        } else {
            crate::ui::view
        };
        let mut rt = Runtime::with_text(
            state,
            view,
            (1440., 900.),
            Env {
                system_dark: true,
                scale: 1.,
            },
            rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
        );
        rt.frame(1.);
        rt.frame(1.);
        rt
    }

    fn offset(rt: &Runtime<State>) -> f32 {
        rt.state.reader_controls.notes_position.get().offset[1]
    }

    fn wheel(rt: &mut Runtime<State>, delta: f32, precise: bool) {
        let (x, y) = rt.find("release-notes").unwrap();
        rt.wheel_event(WheelEvent::new(0., delta).at(x, y).precise(precise));
        rt.frame(1. / 120.);
    }

    fn settle(rt: &mut Runtime<State>) {
        for _ in 0..120 {
            rt.frame(1. / 120.);
        }
    }

    #[test]
    fn notes_notches_glide_without_rebuilding_the_document_on_each_frame() {
        for mode in [NoteMode::Rendered, NoteMode::Source] {
            let mut rt = runtime(mode, false);
            // Count ordinary commits, not Rune's periodic debug stale-view probe.
            rt.check_live_only(false);
            let commits = rune::commit_generation();
            wheel(&mut rt, -120., false);
            let after_event = rune::commit_generation();
            assert!(after_event - commits <= 2, "wheel event caused repeated commits");
            let first = offset(&rt);
            assert!(first > 0. && first < 120., "not a glide: {first}");
            let mut before = first;
            for _ in 0..120 {
                rt.frame(1. / 120.);
                let next = offset(&rt);
                assert!(next + 0.1 >= before && next <= 120.5);
                before = next;
            }
            assert!((before - 120.).abs() < 0.5);
            assert_eq!(
                rune::commit_generation(),
                after_event,
                "animation frames rebuilt the document"
            );
            assert_eq!(rt.state.sidebar_scroll.position.get().offset[1], 0.);
            assert!(!rt.state.live.work.is_pending());
        }
    }

    #[test]
    fn notes_accumulate_reverse_preserve_precise_input_and_reset_after_release_jumps() {
        let mut rt = runtime(NoteMode::Rendered, false);
        for _ in 0..3 {
            wheel(&mut rt, -80., false);
        }
        settle(&mut rt);
        assert!((offset(&rt) - 240.).abs() < 0.5);
        wheel(&mut rt, 40., false);
        settle(&mut rt);
        assert!((offset(&rt) - 200.).abs() < 0.5);
        wheel(&mut rt, -240., false);
        let before = offset(&rt);
        wheel(&mut rt, -20., true);
        let after = offset(&rt);
        assert!((after - before - 20.).abs() < 0.5);
        settle(&mut rt);
        assert!((offset(&rt) - after).abs() < 0.5);
        wheel(&mut rt, -1000., false);
        let (x, y) = rt.find("release-index-item-2").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1. / 120.);
        let jumped = offset(&rt);
        wheel(&mut rt, -40., false);
        settle(&mut rt);
        assert!((offset(&rt) - jumped - 40.).abs() < 0.5);
        let marker = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("release-index-active"))
            .unwrap()
            .rect;
        let selected = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("release-index-item-2"))
            .unwrap()
            .rect;
        assert!(marker[1] >= selected[1] && marker[1] < selected[1] + selected[3]);
        assert!(!rt.state.live.work.is_pending());
    }

    #[test]
    fn notes_respect_reduced_motion_and_a_changed_package_does_not_inherit_a_target() {
        let mut rt = runtime(NoteMode::Rendered, true);
        wheel(&mut rt, -120., false);
        assert!((offset(&rt) - 120.).abs() < 0.5);
        let mut rt = runtime(NoteMode::Rendered, false);
        wheel(&mut rt, -1000., false);
        rt.update(|s, tx| {
            s.select_package("pnpm");
            tx.scroll_to_instant("release-notes", 0., 0.);
        });
        rt.frame(1. / 120.);
        wheel(&mut rt, -40., false);
        settle(&mut rt);
        let at = rt.state.reader_controls.notes_position.get();
        assert!((at.offset[1] - 40_f32.min(at.max[1])).abs() < 0.5);
        assert_eq!(rt.state.selected().unwrap().id, "pnpm");
    }

    #[test]
    fn modified_and_horizontal_wheel_clear_pending_notches_and_pass_through() {
        let mut motion = Motion::default();
        let mut at = ScrollPos::default();
        at.max[1] = 1000.;
        for event in [
            WheelEvent::new(0., -80.).mods(rune::Mods {
                shift: true,
                ..Default::default()
            }),
            WheelEvent::new(0., -80.).mods(rune::Mods {
                ctrl: true,
                ..Default::default()
            }),
            WheelEvent::new(80., 0.),
        ] {
            let mut tx = Tx::default();
            assert!(motion.wheel("notes", at, WheelEvent::new(0., -80.), &mut tx, false));
            assert!(motion.target.is_some());
            assert!(!motion.wheel("notes", at, event, &mut tx, false));
            assert!(motion.target.is_none());
        }
    }
}
