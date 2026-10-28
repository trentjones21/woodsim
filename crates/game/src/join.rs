//! The workbench's state: the assembly on it, the part in hand waiting to go on, and how the
//! last part went on, so it can go on another way or come back off.
//!
//! `woodsim` places each part and measures how the joints fit; this module keeps track of
//! what's on the bench and puts it into words.

use bevy::prelude::*;
use woodsim::{Assembly, Board, Contact, Corner, Fastener, Length, Member, Surface, surfaces};

#[derive(Resource)]
pub struct BenchStation {
    /// The parts put together so far, if any.
    pub assembly: Option<Assembly>,
    pub fastener: Fastener,
    /// How the last part went on.
    last: Option<Placing>,
}

/// How a part went onto the assembly: against which member's surface, the `option`th of the
/// ways it can go there (`Assembly::ways`), lined up at `corner`.
#[derive(Clone, Copy)]
struct Placing {
    onto: (usize, Surface),
    option: usize,
    corner: Corner,
    fastener: Fastener,
}

impl Default for BenchStation {
    fn default() -> BenchStation {
        BenchStation {
            assembly: None,
            fastener: Fastener::Glue,
            last: None,
        }
    }
}

impl BenchStation {
    /// Puts `board` down: on the bare bench, to start an assembly, or against `onto`, a member's
    /// surface, the best way it goes there, held with the chosen fastener. Hands the board back
    /// if it can't go: on a bench with parts on it already, it needs a surface to go against.
    pub fn put(&mut self, board: Board, onto: Option<(usize, Surface)>) -> Result<(), Board> {
        match (&self.assembly, onto) {
            (None, _) => {
                self.assembly = Some(Assembly::of(board));
                self.last = None;
                Ok(())
            }
            (Some(_), None) => Err(board),
            (Some(_), Some(onto)) => self.place(
                board,
                Placing {
                    onto,
                    option: 0,
                    corner: Corner(0),
                    fastener: self.fastener,
                },
            ),
        }
    }

    fn place(&mut self, board: Board, placing: Placing) -> Result<(), Board> {
        let Some(assembly) = &mut self.assembly else {
            return Err(board);
        };
        let ways = assembly.ways(&board, placing.onto);
        let Some(&(surface, way)) = ways.get(placing.option % ways.len().max(1)) else {
            return Err(board);
        };
        let Some(member) = assembly.place(&board, surface, placing.onto, way, placing.corner)
        else {
            return Err(board);
        };
        assembly.join(member, surface, placing.onto, placing.fastener);
        self.last = Some(placing);
        Ok(())
    }

    /// Puts the last part on again, the next way it can go against the same surface.
    pub fn turn_last(&mut self) -> bool {
        self.redo(|placing| placing.option += 1)
    }

    /// Puts the last part on again, lined up at the next corner of the surface it's against.
    pub fn slide_last(&mut self) -> bool {
        self.redo(|placing| placing.corner = Corner((placing.corner.0 + 1) % 4))
    }

    fn redo(&mut self, change: impl FnOnce(&mut Placing)) -> bool {
        let Some(mut placing) = self.last else {
            return false;
        };
        let Some(member) = self.assembly.as_mut().and_then(Assembly::remove_last) else {
            return false;
        };
        change(&mut placing);
        self.place(member.board, placing).is_ok()
    }

    /// Whether `member` is the last part that went on, the one that can come back off.
    pub fn is_last(&self, member: usize) -> bool {
        self.assembly
            .as_ref()
            .is_some_and(|assembly| member + 1 == assembly.members().len())
    }

    /// Takes the last part back off the assembly, or the only part off the bench.
    pub fn take_last(&mut self) -> Option<Board> {
        let assembly = self.assembly.as_mut()?;
        self.last = None;
        match assembly.remove_last() {
            Some(member) => Some(member.board),
            None => self
                .assembly
                .take()
                .map(|assembly| assembly.members()[0].board.clone()),
        }
    }

    /// How the joints fit: the ones that aren't tight, worst first, or that they all are.
    pub fn fit(&self) -> String {
        let Some(assembly) = &self.assembly else {
            return String::new();
        };
        let mut loose: Vec<Contact> = assembly
            .fit()
            .into_iter()
            .filter(|contact| !contact.is_tight())
            .collect();
        if loose.is_empty() {
            return if assembly.members().len() > 1 {
                "Every joint is tight.".to_string()
            } else {
                String::new()
            };
        }
        let worst = |contact: &Contact| contact.least.as_nm().abs().max(contact.most.as_nm().abs());
        loose.sort_by_key(|contact| std::cmp::Reverse(worst(contact)));
        loose
            .iter()
            .map(|contact| {
                let [(a, _), (b, _)] = contact.between;
                let (least, most) = (measured(contact.least), measured(contact.most));
                let how = if contact.most.as_nm() < 0 {
                    format!("runs into it by {}", measured(-contact.least))
                } else if contact.least.as_nm() < 0 {
                    format!("runs into it by {} at one end", measured(-contact.least))
                } else if least == most {
                    format!("is open {most}")
                } else {
                    format!("is open {least} to {most}")
                };
                format!("Part {} {how} where it meets part {}.", b + 1, a + 1)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// One line describing what's on the bench, given what's in hand.
    pub fn status(&self, holding: Option<&Board>) -> String {
        let fit = self.fit();
        let fit = if fit.is_empty() {
            String::new()
        } else {
            format!(" {fit}")
        };
        match (&self.assembly, holding) {
            (None, None) => {
                "Nothing on the bench. Pick up a part and click the bench to set it down."
                    .to_string()
            }
            (None, Some(_)) => {
                "Click the bench to set it down and start putting parts together.".to_string()
            }
            (Some(_), Some(_)) => {
                "Click where on the others it goes. R tries it another way.".to_string()
            }
            (Some(_), None) if self.last.is_none() => {
                "Pick up another part and click where it goes on this one.".to_string()
            }
            (Some(assembly), None) => {
                let fastener =
                    fastened(self.last.map_or(self.fastener, |placing| placing.fastener));
                match assembly.joints().last() {
                    Some(joint) => {
                        let [(onto, theirs), (part, mine)] = joint.between;
                        format!(
                            "Part {}'s {} is against part {}'s {}, with {fastener}.{fit}",
                            part + 1,
                            name(mine),
                            onto + 1,
                            name(theirs)
                        )
                    }
                    None => format!("Part 1 is on the bench.{fit}"),
                }
            }
        }
    }
}

/// A length as a tape measure reads it, to the nearest 64th.
fn measured(length: Length) -> Length {
    length.rounded_to(Length::from_inch_fraction(1, 64))
}

/// Turns the assembly's axes into the bench's: the assembly's `x` runs along the bench to the
/// right, its `y` away from the operator, and its `z` up.
fn onto_bench(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[2], -v[1])
}

/// How a member sits on the bench: the rotation that turns its mesh (`w`, `t`, `l` along x,
/// y, z) into place, and where its corner is, in meters from the assembly's origin.
pub fn member_pose(member: &Member) -> (Quat, Vec3) {
    let [l, w, t] = member.axes().map(|axis| onto_bench(axis.map(|v| v as f32)));
    let rotation = Quat::from_mat3(&Mat3::from_cols(w, t, l));
    (rotation, onto_bench(member.at.map(Length::as_meters_f32)))
}

/// The space the assembly takes up on the bench, as its near and far corners in meters from
/// its origin.
pub fn bounds(assembly: &Assembly) -> (Vec3, Vec3) {
    assembly
        .members()
        .iter()
        .flat_map(|member| {
            let board = &member.board;
            (0..8).map(move |k| {
                let pick = |bit: usize, extent: Length| {
                    if k & bit == 0 { Length::ZERO } else { extent }
                };
                let corner = member.point(
                    pick(1, board.length()),
                    pick(2, board.width()),
                    pick(4, board.thickness()),
                );
                onto_bench(corner.map(Length::as_meters_f32))
            })
        })
        .fold((Vec3::MAX, Vec3::MIN), |(low, high), corner| {
            (low.min(corner), high.max(corner))
        })
}

/// The surface of `board` at `point`, with the surface facing `normal` there, both in the
/// board's mesh axes (`w`, `t`, `l` along x, y, z, in meters).
pub fn surface_at(board: &Board, point: Vec3, normal: Vec3) -> Option<Surface> {
    let normal = normal.normalize_or_zero();
    if normal.y.abs() > 0.9 {
        return Some(if normal.y > 0.0 {
            Surface::Top
        } else {
            Surface::Bottom
        });
    }
    // The side facing the same way whose plane the point is nearest.
    surfaces(board)
        .into_iter()
        .filter_map(|surface| {
            let Surface::Side(edge) = surface else {
                return None;
            };
            let out = Vec2::new(edge.out.w as f32, edge.out.l as f32);
            let scale = out.length();
            let facing = out.dot(Vec2::new(normal.x, normal.z)) / scale;
            let plane = edge.at as f64 / scale as f64 / 1e9;
            let off = (out.dot(Vec2::new(point.x, point.z)) / scale) as f64 - plane;
            (facing > 0.95 && off.abs() < 0.002).then_some((surface, off.abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(surface, _)| surface)
}

/// How a joint is held: "glue", "brads", "screws".
pub fn fastened(fastener: Fastener) -> &'static str {
    match fastener {
        Fastener::Glue => "glue",
        Fastener::Brads => "brads",
        Fastener::Screws => "screws",
    }
}

/// What a board's surface is called: a face, an edge, an end, or a mitered end.
pub fn name(surface: Surface) -> &'static str {
    match surface {
        Surface::Bottom => "bottom face",
        Surface::Top => "top face",
        Surface::Side(edge) if edge.out.l != 0 && edge.out.w != 0 => "mitered end",
        Surface::Side(edge) if edge.out.l != 0 => "end",
        Surface::Side(_) => "edge",
    }
}

#[cfg(test)]
mod tests {
    use woodsim::{Angle, Length, MiterSaw, Orientation, Pass, surfaces};

    use super::*;

    /// A frame piece 10" long point to long point, mitered 45° at both ends.
    fn frame_piece() -> Board {
        let stock = Board::new(
            Length::from_inches(48),
            Length::from_inch_fraction(3, 2),
            Length::from_inch_fraction(3, 4),
        );
        let saw = |degrees| MiterSaw {
            miter: Angle::from_degrees(degrees),
            ..MiterSaw::twelve_inch()
        };
        let Ok(Pass::Cut(trim)) =
            saw(-45).crosscut(&stock, Orientation::default(), Length::from_inches(2))
        else {
            panic!("expected the end trimmed");
        };
        let Ok(Pass::Cut(cut)) = saw(45).crosscut(
            &trim.offcut.unwrap().board,
            Orientation::default(),
            Length::from_inches(10),
        ) else {
            panic!("expected a frame piece");
        };
        cut.piece.board
    }

    fn miter(board: &Board, far_end: bool) -> Surface {
        surfaces(board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.w != 0 && (edge.out.l > 0) == far_end)
            })
            .unwrap()
    }

    #[test]
    fn the_first_part_goes_on_the_bare_bench_and_the_next_against_it() {
        let mut bench = BenchStation::default();
        assert_eq!(bench.put(frame_piece(), None), Ok(()));
        // With a part on the bench, the next one needs a surface to go against.
        assert!(bench.put(frame_piece(), None).is_err());
        let piece = frame_piece();
        assert_eq!(
            bench.put(piece.clone(), Some((0, miter(&piece, true)))),
            Ok(())
        );
        assert_eq!(bench.assembly.as_ref().unwrap().members().len(), 2);
    }

    #[test]
    fn four_frame_pieces_go_together_tight() {
        let mut bench = BenchStation::default();
        let piece = frame_piece();
        bench.put(piece.clone(), None).unwrap();
        for last in 0..3 {
            // The piece's matching miter goes against the last one's, face up, by itself.
            bench
                .put(piece.clone(), Some((last, miter(&piece, true))))
                .unwrap();
        }
        assert_eq!(bench.assembly.as_ref().unwrap().members().len(), 4);
        assert_eq!(
            bench.status(None),
            "Part 4's mitered end is against part 3's mitered end, with glue. Every joint is \
             tight."
        );
    }

    #[test]
    fn the_last_part_can_go_on_another_way_or_come_back_off() {
        let mut bench = BenchStation::default();
        let piece = frame_piece();
        bench.put(piece.clone(), None).unwrap();
        bench
            .put(piece.clone(), Some((0, miter(&piece, true))))
            .unwrap();
        let first = bench.assembly.as_ref().unwrap().members()[1].clone();
        assert!(bench.slide_last());
        assert!(bench.turn_last());
        assert_ne!(bench.assembly.as_ref().unwrap().members()[1], first);

        // Only the last part on comes back off, then the one under it.
        assert!(!bench.is_last(0) && bench.is_last(1));
        assert_eq!(bench.take_last(), Some(piece.clone()));
        assert_eq!(bench.take_last(), Some(piece));
        assert!(bench.assembly.is_none());
    }

    #[test]
    fn a_frame_that_does_not_close_says_where() {
        let mut bench = BenchStation::default();
        let piece = frame_piece();
        let long = {
            let mut longer = piece.clone();
            // The same piece cut 1/16" long, from the same saw setup.
            let stock = Board::new(
                Length::from_inches(48),
                Length::from_inch_fraction(3, 2),
                Length::from_inch_fraction(3, 4),
            );
            let saw = |degrees| MiterSaw {
                miter: Angle::from_degrees(degrees),
                ..MiterSaw::twelve_inch()
            };
            if let Ok(Pass::Cut(trim)) =
                saw(-45).crosscut(&stock, Orientation::default(), Length::from_inches(2))
                && let Ok(Pass::Cut(cut)) = saw(45).crosscut(
                    &trim.offcut.unwrap().board,
                    Orientation::default(),
                    Length::from_inches(10) + Length::from_inch_fraction(1, 16),
                )
            {
                longer = cut.piece.board;
            }
            longer
        };
        bench.put(piece.clone(), None).unwrap();
        for (last, next) in [piece.clone(), long, piece.clone()].into_iter().enumerate() {
            let onto = miter(
                &bench.assembly.as_ref().unwrap().members()[last].board,
                true,
            );
            bench.put(next, Some((last, onto))).unwrap();
        }
        assert_eq!(bench.fit(), "Part 4 is open 3/64\" where it meets part 1.");
    }

    #[test]
    fn the_first_member_lies_along_the_bench_face_up() {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        let assembly = Assembly::of(board.clone());
        let (rotation, corner) = member_pose(&assembly.members()[0]);
        assert_eq!(corner, Vec3::ZERO);
        // Its length runs along the bench, its width away from the operator, and its
        // thickness up.
        assert!((rotation * Vec3::Z).abs_diff_eq(Vec3::X, 1e-6));
        assert!((rotation * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-6));
        assert!((rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-6));
        let (near, far) = bounds(&assembly);
        assert!((far - near).abs_diff_eq(Vec3::new(0.6096, 0.01905, 0.1016), 1e-6));
    }

    #[test]
    fn a_click_finds_the_surface_under_it() {
        let piece = frame_piece();
        let thickness = piece.thickness().as_meters_f32();
        // The top, from above.
        assert_eq!(
            surface_at(&piece, Vec3::new(0.01, thickness, 0.1), Vec3::Y),
            Some(Surface::Top)
        );
        // The mitered end at `l = 0`: its long point is at the `w = width` edge, so it faces
        // back along the length and out toward `w = 0`.
        let width = piece.width().as_meters_f32();
        let normal = Vec3::new(-1.0, 0.0, -1.0).normalize();
        let on_miter = Vec3::new(width / 2.0, thickness / 2.0, width / 2.0);
        assert_eq!(
            surface_at(&piece, on_miter, normal),
            Some(miter(&piece, false))
        );
        // The long edge.
        assert_eq!(
            surface_at(&piece, Vec3::new(width, 0.005, 0.1), Vec3::X).map(name),
            Some("edge")
        );
    }
}
