# Surface-element aerodynamics

`src/aero.rs` flies an aircraft on strips: the wing, the tailplane and the fin
cut spanwise, each in its own local airflow, the forces and moments summed
about the centre of gravity. Damping, stability, stalls, wing drop, propeller
slipstream and swirl, and what damage does all come out of the strips rather
than from tuned gains. Coefficients cover only what strips cannot see: the
fuselage's own pitching and yawing moments and side force, and the roll due to
sideslip of the wing-body junction.

An aircraft flies on it when its `Spec` has a `rig`. Without one it keeps the
older whole-aircraft model (`lift_drag` plus the `handling` gains), so aircraft
move over one at a time.

## What a rig takes

Nothing in the code is aircraft-specific: an aircraft is data.

From the `Spec` (already there for every aircraft):

- the wing: `wing_sect` (leading edge, chord, incidence and dihedral at each
  span station), `span`, `wing_area`;
- the aileron span: `aileron`; the flap notches: `flaps`;
- the parasite drag: `cd0` (what the sections' own profile drag does not
  cover is given to the body), and `cd_gear`, `cd_flap`, `spoiler_cd`;
- mass, inertia and the CG (`cg`, `mac` and the loading) — the strips' moments
  are taken about the CG the load puts there, so loading changes the handling;
- the engine: a piston's propeller diameter, for its slipstream and torque.

From the `Rig` (`aero::Rig`):

| field | what |
|---|---|
| `wing_foil` | the wing section: 2-D lift slope, zero-lift angle, cl_max, cd0, cm0 |
| `wing_strips`, `oswald` | strips per half, span efficiency |
| `flap`, `flap_dcl_max` | flap span and chord fraction; extra cl_max fully down |
| `aileron_chord`, `aileron_travel` | chord fraction; travel up, down (rad) |
| `elevons` | tailless (a delta): the trailing edge does pitch and roll |
| `htail`, `htail_foil`, `elevator` | tailplane planform (quarter-chord line, chords, incidence), section, elevator (chord 1 for a stabilator) |
| `trim` | the trim's range as elevator deflection, nose down and up |
| `fin`, `fin_foil`, `rudder` | the same for the fin |
| `fuse` | fuselage coefficients (`cm_a`, `cn_b`, `cl_b`, `cy_b`) |
| `downwash`, `eta_tail` | share of the lifting-line downwash at the tail; tail dynamic pressure ratio |
| `prop_hub`, `swirl` | the propeller's hub and its slipstream's swirl |

The aileron and rudder rigging that makes the aircraft fly straight hands off
in the cruise (its fixed tabs and fin offset) is worked out at spawn from
`Spec::cruise`, as is the trim for the speed it starts at.

## Rigging a new aircraft

1. Start from `aero::Rig::derive(spec)`. It builds a rig from what the `Spec`
   already says: the section's lift and stall from the aircraft polar, the
   tailplane and fin from `Spec::tail`'s tips sized by typical tail volume
   coefficients, typical control travel, elevons if the "stabiliser tip" is on
   the wing, a stabilator for an agile jet. The test
   `every_aircraft_can_be_rigged_from_its_spec` flies every aircraft on its
   derived rig.
2. Replace the estimates with the real aircraft's figures: tail planforms from
   drawings, sections, control travel from the type certificate data sheet.
   Record the sources in comments, as `c172::RIG` does.
3. Set `rig: Some(&RIG)` on the `Spec`.
4. Check it: the aircraft's `performance_is_*` test, the handling harness
   (`cargo test --release --bin fablegame modes:: -- --nocapture`) against the
   reference values it prints, and a stall test like
   `a_cessna_172_stalls_where_the_book_says`. Then fly it.

## Trim

With a rig, the trim is a real elevator trim (`Plane::pitch_trim`, -1 nose down
to 1 nose up): the trim keys and the MCP `trim_deg` move it. The auto-trim
assist (Settings, on by default) winds it while the pilot flies the stick, to
where the aircraft holds 1 g hands off at its speed, as the older model's
auto-trim does. With it off, trimming is the pilot's job.

## Limits

- Strip theory: no spanwise flow, no vortex lattice. The 3-D lift slope and
  induced drag come from each surface's aspect ratio.
- The downwash is the lifting-line value, lagged by the time the air takes to
  reach the tail; the slipstream is momentum theory with a fixed swirl ratio.
- Compressibility is only the `Spec`'s wave drag; no Mach effect on the sections yet.
- A delta's vortex lift and the derived Phantom do not trim yet; they need rigs of their own.
