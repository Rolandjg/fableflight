# Wind, turbulence and the air around the aircraft

Everything the air does to an aircraft beyond its own motion. None of it is
aircraft-specific: an aircraft is only geometry (`Spec`) and the air is only
weather and terrain, so every aircraft, built-in or downloaded, gets all of it.

| what | where | driven by |
|---|---|---|
| mean wind and its profile with height | `weather.rs` `Weather::profile` / `steady` | surface wind, stability, live winds aloft |
| gusts (slow swells of the mean wind) | `weather.rs` `Weather::at` | the Gusts slider / METAR gust factor |
| turbulence intensity and scale | `weather.rs` `Weather::air` | wind, convection, cloud, storms |
| turbulence as a 3D field | `gust.rs` `GustField` | the intensity and scale above |
| thermals, storm drafts | `weather.rs`, `storm.rs` | sun, clouds, storm cells |
| ridge lift, lee waves, rotor | `orographic.rs` | the world's terrain, wind, stability |
| wake vortices | `wake.rs` | airport and live traffic |
| how the airframe feels all that | `plane.rs` `Plane::turbulence` (and `strip_gusts` on the surface-element core) | the aircraft's geometry |

## The wind profile

`Weather::profile(lat, alt, agl)` gives speed and direction at any height.

- **Boundary layer.** From the 10 m wind as a power law; exponent 0.11
  (sunny afternoon) to 0.38 (clear calm night), reaching 1.4 to 2.3 times the
  10 m wind at the layer top. The layer top is the convective mixed layer by
  day and a few hundred metres on a stable night. `Weather::stability` goes
  from 0 (convective) to 1 (stable); a strong wind stirs it towards neutral.
- **Veer.** The direction turns clockwise with height through the layer in
  the northern hemisphere (counter-clockwise in the southern), 12° to 35°
  from the surface to the top as it gets more stable.
- **Low-level jet.** On a stable night a wind maximum above the layer-top
  wind in the middle of the layer.
- **Aloft.** With live weather, Open-Meteo's model winds at 850, 700, 500,
  300 and 250 hPa (`livewx.rs` `winds_aloft`), interpolated in components and
  eased in when a new report arrives. Without them: the layer-top wind growing
  about 2 m/s per km at mid latitudes (less in the tropics, half as much on a
  calm day) to the tropopause near 11 km, falling off above it.

## Turbulence: a field the aircraft flies through

`GustField` is a frozen von Kármán gust field: a sum of random Fourier modes
(log-spaced wavenumber shells from 6 km to 2 m wavelength, two orthogonal
triads per shell, each wave's velocity perpendicular to its wavevector so the
air is incompressible). The shells' amplitudes follow the von Kármán spectrum
for the horizontal and vertical intensity and length scale `Weather::air`
asks for (MIL-HDBK-1797 forms), so changing them never moves the waves and
the field never jumps. It drifts with the mean wind, and its eddies turn over
slowly so still air still gusts. Eddies much smaller than the wing chord are
tapered by the Sears function, as a real wing barely responds to them.

Because it is a field, it is sampled where each part of the airframe is:

- **Every aircraft** samples it at the CG, four stations along each wing, the
  nose and the tail (`gust_probes`, from `wing_sect`, `contacts` and
  `tail.fin_base`). The mean over the wing is the gust; the fit of its
  variation along the span and the fuselage gives the air's rolling, pitching
  and yawing (`gust_rot`). The roll gust this gives matches MIL-F-8785C's
  (σ_p² = σ_w² 0.8 π² (πL/4b)^(1/3) / (8 b L)) within about 10% from a C172
  to a 747 (`gust_gradients_follow_the_airframe`), without per-aircraft tuning.
- **Aircraft on the surface-element core** (`aero.rs`) sample it at every
  strip of the wing, tailplane and fin (`strip_gusts`), so each strip flies in
  its own gust and the rolling, pitching and yawing come out of the strips.

Turbulence bursts (patches of rough air, `burst`) scale the whole field up
and down, lognormal with the mean variance unchanged.

## Terrain: ridge lift, lee waves and rotor

`Orography` reads the ground along the wind line through the aircraft (50 km
upwind to 1 km downwind, every 200 m) once a second or every 250 m flown.

- **Slope flow**: w = U ∂h/∂s at the surface, fading with height over a
  depth set by the local relief. Lift on the windward side, sink in the lee.
- **Lee waves** (linear theory): every bit of upwind slope forces the air and
  in stable air the response is a train of waves of length λ = 2πU/N (U the
  wind about a kilometre above the ridge tops, N from the stability),
  decaying slowly downstream, saturating at 0.6 U or 12 m/s. A ridge about a
  quarter to half a wavelength wide excites them best. They need wind across
  the ridge from about 7 m/s and fade out above the tropopause.
- **Rotor**: under strong waves, below the ridge tops, extra turbulence with
  eddies a few hundred metres across.

## Wake turbulence

`Wakes` keeps a trail for every other aircraft within 20 km (airport traffic
in the climb and on final, live traffic, other players in multiplayer): Γ₀ = m g / (ρ V b₀), b₀ = π/4 b,
Burnham-Hallock swirl, sinking at Γ/(2π b₀), stopping and spreading apart
near the ground, drifting with the wind, decaying after a while (sooner in
turbulent air). Traffic masses come from the size class (`airmodels::Type::mass`), players' from their aircraft's `Spec`.
The vortices' velocity is added wherever the airframe samples the air, so a
light aircraft that flies a wing into a heavy's vortex rolls hard and a big
one much less (`a_heavy_jets_wake_rolls_a_light_aircraft`: a C172 hands off
reaches about 80° of bank in 4 s, a 747 about 14°).

## Limits

- The gust field is homogeneous over the airframe; intensity and scale come
  from the aircraft's position, not each strip's.
- Lee waves are 2-D along the wind line through the aircraft, from linear
  theory with a single stability; no hydraulic jumps or downslope windstorms.
- The terrain profile only exists where the world has loaded ground.
- Airport traffic sheds a wake only above 10 m (the traffic model's own
  limit); a player's wake is laid from 20 m/s, so also on a fast takeoff roll.
- `Weather::at`'s gust swells are uniform in space (they stand for the
  reported gust factor) and sit on top of the field.
