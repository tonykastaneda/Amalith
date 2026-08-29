use amalith_commands::{Command, CommandError, CommandOutcome, Editor};
use amalith_core::{ObjectKind, PathData, Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrimitiveKind {
    RoundedRectangle,
    Polygon,
    Star,
}

impl PrimitiveKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::RoundedRectangle => "Rounded Rectangle",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
        }
    }
}

#[derive(Default)]
pub(crate) struct PrimitiveTool {
    pub(crate) active: Option<PrimitiveKind>,
    pub(crate) preview: Option<PathData>,
    start: Option<Point>,
}

impl PrimitiveTool {
    pub(crate) fn set_active(&mut self, kind: Option<PrimitiveKind>) {
        self.active = kind;
        if kind.is_none() {
            self.cancel_drag();
        }
    }

    pub(crate) fn begin_drag(&mut self, point: Point) {
        self.start = Some(point);
        self.preview = None;
    }

    pub(crate) fn update_drag(&mut self, point: Point, constrain: bool) {
        let (Some(start), Some(kind)) = (self.start, self.active) else {
            return;
        };
        let rect = constrained_rect(start, point, constrain);
        self.preview = Some(match kind {
            PrimitiveKind::RoundedRectangle => {
                PathData::rounded_rectangle(rect, rect.width().min(rect.height()) * 0.18)
            }
            PrimitiveKind::Polygon => regular_polygon(rect, 6),
            PrimitiveKind::Star => star(rect, 5, 0.45),
        });
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.start = None;
        self.preview = None;
    }

    pub(crate) fn finish_drag(&mut self, editor: &mut Editor) -> Result<bool, CommandError> {
        self.start = None;
        let Some(path) = self.preview.take() else {
            return Ok(false);
        };
        let Some(kind) = self.active else {
            return Ok(false);
        };
        let bounds = path.local_bounds();
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return Ok(false);
        }
        let layer = if let Some(layer) = editor.document().layers().last() {
            layer.id
        } else {
            match editor.execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })? {
                CommandOutcome::Layer(id) => id,
                _ => unreachable!(),
            }
        };
        editor.execute(Command::CreatePath {
            layer,
            path,
            name: Some(next_name(editor, kind)),
        })?;
        Ok(true)
    }
}

fn constrained_rect(start: Point, end: Point, constrain: bool) -> Rect {
    let end = if constrain {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let side = dx.abs().max(dy.abs());
        Point::new(start.x + side.copysign(dx), start.y + side.copysign(dy))
    } else {
        end
    };
    Rect::new(
        start.x.min(end.x),
        start.y.min(end.y),
        start.x.max(end.x),
        start.y.max(end.y),
    )
}

fn regular_polygon(rect: Rect, sides: usize) -> PathData {
    let center = rect.center();
    let rx = rect.width() * 0.5;
    let ry = rect.height() * 0.5;
    let points: Vec<_> = (0..sides)
        .map(|index| {
            let angle =
                -std::f64::consts::FRAC_PI_2 + index as f64 * std::f64::consts::TAU / sides as f64;
            Point::new(center.x + rx * angle.cos(), center.y + ry * angle.sin())
        })
        .collect();
    PathData::polygon(&points)
}

fn star(rect: Rect, points_count: usize, inner_ratio: f64) -> PathData {
    let center = rect.center();
    let rx = rect.width() * 0.5;
    let ry = rect.height() * 0.5;
    let points: Vec<_> = (0..points_count * 2)
        .map(|index| {
            let angle = -std::f64::consts::FRAC_PI_2
                + index as f64 * std::f64::consts::PI / points_count as f64;
            let ratio = if index % 2 == 0 { 1.0 } else { inner_ratio };
            Point::new(
                center.x + rx * ratio * angle.cos(),
                center.y + ry * ratio * angle.sin(),
            )
        })
        .collect();
    PathData::polygon(&points)
}

fn next_name(editor: &Editor, kind: PrimitiveKind) -> String {
    let prefix = kind.name();
    let highest = editor
        .document()
        .objects()
        .filter(|object| matches!(object.kind, ObjectKind::Path(_)))
        .filter_map(|object| object.name.as_deref())
        .filter_map(|name| {
            name.strip_prefix(prefix)?
                .trim_start()
                .parse::<usize>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    format!("{prefix} {}", highest + 1)
}
