use amalith_shell::{
    blenddlg,
    canvas::CanvasView,
    dock::{DockModel, PanelId, PanelKind, Side},
    layout, metrics, offsetdlg, panels, picker, shapedialog,
    text::TextContext,
    theme::Theme,
    tool::Tool,
    workspace::Layout,
    xformdlg,
};
use vello::kurbo::{Affine, Point, Rect};

struct ResetScale;
impl Drop for ResetScale {
    fn drop(&mut self) {
        metrics::apply(1.0);
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
}

#[test]
fn panel_and_dialog_content_heights_grow_with_their_windows() {
    let _reset = ResetScale;
    let dimensions = |scale| {
        metrics::apply(scale);
        let mut sizes = vec![
            picker::metric_w(),
            picker::metric_h(),
            blenddlg::body_height(),
            offsetdlg::body_height(),
            xformdlg::body_height(xformdlg::Kind::Reflect),
            xformdlg::body_height(xformdlg::Kind::Shear),
        ];
        for tool in [
            Tool::Rectangle,
            Tool::RoundedRect,
            Tool::Ellipse,
            Tool::Polygon,
            Tool::Star,
            Tool::Arc,
            Tool::Spiral,
        ] {
            sizes.push(shapedialog::body_height(tool));
        }
        for id in [
            PanelKind::Tools,
            PanelKind::Character,
            PanelKind::Paragraph,
            PanelKind::Transform,
            PanelKind::Align,
            PanelKind::Pathfinder,
            PanelKind::Gradient,
            PanelKind::Color,
            PanelKind::Layers,
            PanelKind::Links,
            PanelKind::Artboards,
            PanelKind::Swatches,
        ] {
            sizes.push(panels::min_body_height(PanelId(id), 264.0 * scale));
        }
        sizes
    };
    let base = dimensions(1.0);
    for scale in [1.25, 1.5] {
        for (i, (actual, baseline)) in dimensions(scale).iter().zip(&base).enumerate() {
            assert!(
                (actual - baseline * scale).abs() < 1e-4,
                "dimension {i}: {actual} != {}",
                baseline * scale
            );
        }
    }
}

#[test]
fn text_scales_once_and_document_coordinates_do_not_change() {
    let mut text = TextContext::new();
    let expected = text.measure("Preferences 125%", 18.0);
    let view = CanvasView::default();
    let before = view.to_screen() * Point::new(120.0, 240.0);
    let document_text = amalith_core::TextData {
        content: "Document text".into(),
        ..Default::default()
    };
    let outline = amalith_shell::textedit::outline_text_data(&document_text, &mut text);
    text.set_ui_scale(1.5);
    assert_eq!(
        outline,
        amalith_shell::textedit::outline_text_data(&document_text, &mut text)
    );
    close(text.measure("Preferences 125%", 12.0), expected);
    assert_eq!(before, view.to_screen() * Point::new(120.0, 240.0));
    text.set_ui_scale(1.0);
    let base = text.wrap(
        "one two three four five six",
        12.0,
        vello::peniko::Color::WHITE,
        75.0,
        1.3,
    );
    text.set_ui_scale(1.5);
    let scaled = text.wrap(
        "one two three four five six",
        12.0,
        vello::peniko::Color::WHITE,
        112.5,
        1.3,
    );
    assert_eq!(base.lines().count(), scaled.lines().count());
    close(scaled.height() as f64, base.height() as f64 * 1.5);
}

#[test]
fn layout_geometry_and_saved_workspaces_do_not_compound_scale() {
    let _reset = ResetScale;
    let mut dock = DockModel::new();
    let id = dock.spawn_master(
        vec![vec![PanelId(PanelKind::Color), PanelId(PanelKind::Layers)]],
        [30.0, 40.0, 264.0, 400.0],
    );
    dock.dock_master(id, Side::Right, 0);
    let snapshot = Layout::capture(&dock, true, false, false, None);
    let theme = Theme::default();
    let baseline = layout::layout_master(
        dock.master(id).unwrap(),
        Rect::new(0.0, 0.0, 264.0, 400.0),
        &theme,
        &mut |_| 80.0,
        false,
        true,
    );
    for scale in [1.25, 1.5, 1.0] {
        metrics::apply(scale);
        snapshot.apply_to(&mut dock);
        let mut theme = Theme::default();
        theme.set_ui_scale(scale);
        let m = dock.master(id).unwrap();
        close(m.rect[2] as f64, 264.0 * scale);
        assert_eq!(m.rect[..2], [30.0, 40.0]);
        let frame = layout::layout_master(
            m,
            Rect::new(0.0, 0.0, 264.0 * scale, 400.0 * scale),
            &theme,
            &mut |_| 80.0 * scale,
            false,
            true,
        );
        for (a, b) in [
            (frame.header, baseline.header),
            (frame.close, baseline.close),
            (frame.groups[0].tab_strip, baseline.groups[0].tab_strip),
            (
                frame.groups[0].tabs[0].rect,
                baseline.groups[0].tabs[0].rect,
            ),
        ] {
            let scaled = Affine::scale(scale).transform_rect_bbox(b);
            close(a.x0, scaled.x0);
            close(a.y0, scaled.y0);
            close(a.x1, scaled.x1);
            close(a.y1, scaled.y1);
        }
        let saved = Layout::capture(&dock, true, false, false, None);
        assert_eq!(saved.masters, snapshot.masters);
    }
}

#[test]
fn invalid_preferences_cannot_create_invalid_geometry() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(metrics::normalize_scale(bad), 1.0);
    }
    assert_eq!(metrics::normalize_scale(-2.0), 1.0);
    assert_eq!(metrics::normalize_scale(30.0), 1.5);
}
