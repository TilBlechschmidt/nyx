# Validation
The validation process ensures that the dynamics are correctly implemented.

Many of the features of this library are validated against [GMAT](https://software.nasa.gov/software/GSC-17177-1) ([wiki](http://gmatcentral.org/display/GW/GMAT+Wiki+Home)), version 2017a.
GMAT is validated on flown missions. It was also validated against other software, cf. [this PDF](./tests/GMAT_scripts/GMAT_V&V_ProcessAndResults.pdf).

- [Propagators](#propagators)
  * [With a fixed step](#fixed-step)
  * [Adaptive step](#adaptive-step)
- [Orbital state](#orbital-state)
  * [From a Cartesian state](#from-a-cartesian-state)
  * [From a Keplerian state](#from-a-keplerian-state)
- [Multibody dynamics](#multibody-dynamics)

# Propagators
The purpose of this test is solely to test the correct implementation of the propagator coefficients, error computation, and adaptive step size. The algorithms were taken from GMAT unless noted otherwise in the source code.
The tests in [`propagators.rs`](./tests/propagators.rs) we executed in GMAT as well. Each script is in the subfolder [propagators](./tests/GMAT_scripts/propagators/) and is named after the propagator used. In addition to the GMAT tests, backward propagation is confirmed for two body dynamics using an adaptive step-size: all propagation tests are verified to return to their initial states with an error in position less than 1e-5 km and an error in velocity less than 1e-8 km/s.

The validation scenario is simply a two body propagation around Earth for one day.
The following tables corresponds to the **errors** between the `nyx` output for the one day LEO propagation in two body dynamics and the GMAT output using the same step configuration.

## Adaptive step

Step size configured as follows:
+ Minimum step: 0.1 seconds
+ Maximum step: 30.0 seconds
+ Accuracy: 1e-12
+ Max attempts: 50

*Note:* GMAT uses the secant method to determine the final step to take (as a fixed step). In this validation effort, we compute the exact number of seconds by which we overshot the expected elapsed time, and set the final step size to precisely that value. As such, the following table has the extra column `Δtime` which shows the difference in the propagation duration between GMAT and `nyx`. As explained, the latter will propagate for _exactly_ 86400 seconds (to machine precision), but GMAT doesn't, and this is mostly to blame for the (slight) differences in final states.

Propagator  | x | y | z | vx | vy | vz | Δtime
--|---|---|---|---|---|---|--
Dormand45  | 3.0e-07 | 9.2e-06 | 1.3e-05 | 1.1e-08 | 7.5e-09 | 5.3e-09 | 0.0
Verner56  | 6.4e-09 | 1.7e-07 | 2.5e-07 | 2.3e-10 | 1.5e-10 | 1.1e-10 | 1e-11
Dormand78  | 2.5e-10 | 9.7e-09 | 1.3e-08 | 1.2e-11 | 7.9e-12 | 5.6e-12 | 1e-11
RK89  | 3.9e-10 | 7.7e-09 | 1.0e-08 | 2.9e-12 | 6.4e-12 | 4.4e-12 | 0.0

## Fixed step

Step size set to 10.0 seconds.

Propagator  | x | y | z | vx | vy |  vz
--|---|---|---|---|---|--
RK4Fixed  | 1.4e-9 | 3.0e-8 | 4.0e-8 | 3.6e-11 | 2.4e-11 | 1.7e-11
Dormand45  | 2.9e-08 | 8.5e-07 | 1.2e-06 | 1.0e-09 | 7.0e-10 | 4.9e-10
Verner56  | 3.8e-10 | 3.9e-09 | 5.3e-09 | 4.8e-12 | 3.4e-12 | 2.1e-12
Dormand78  | 3.9e-10 | 1.2e-08 | 1.7e-08 | 1.5e-11 | 1.0e-11 | 7.0e-12
RK89  | 3.8e-11 | 2.3e-09 | 3.2e-09 | 2.9e-12 | 1.9e-12 | 1.5e-12

# Orbital state
This validation compares the computations of orbital elements in nyx with those in GMAT. Each scenario script is in the subfolder [state](./tests/GMAT_scripts/state/).
The validation is trivially the following: a spacecraft is defined in GMAT in the EarthMJ2000Eq frame by its Cartesian state. There is a "Propagate" segment of strictly zero seconds. The desired orbital state computations are exported to a report file.

The following table corresponds to the **absolute errors** between the `nyx` computations and those of GMAT. Note that if `nyx` returned more significant digits than GMAT and all the GMAT digits matched those of nyx, the error is also marked as zero (this was only seen for the energy and orbital period where GMAT returns 13 digits and `nyx` 14).

## From a Cartesian state

Element / Scenario  | circular inclined  | circular equatorial  | elliptical
--|---|---|--
Earth.Energy  | 0.0 | 0.0 | 0.0
Earth.OrbitPeriod | 0.0 | 0.0 | 0.0
Earth.HX  | 7e-12  | 0.0 | 3e-12
Earth.HY  | 7e-12  | 1e-16 | 0.0
Earth.HZ  | 0.0  | 0.0 | 0.0
Earth.SMA  | 0.0  | 0.0 | 0.0
Earth.ECC  |  0.0 | 1e-17 | 5e-17
EarthMJ2000Eq.INC  | 0.0 | 0.0  | 0.0
EarthMJ2000Eq.RAAN  | 0.0  | 0.0  | 0.0
EarthMJ2000Eq.AOP  | 0.0 | 3e-7 | 5e-11
Earth.TA  | 0.0 | 3e-7 | 4e-14
Earth.TLONG | 0.0 | 3e-14 | **1e-8**<sup>(1)</sup>
Earth.EA | 0.0 | 0.0 | 0.0
Earth.MA | 0.0 | 0.0 | 0.0
Earth.RadApo | 0.0 | 0.0 | 0.0
Earth.RadPer | 0.0 | 0.0 | 0.0
Earth.SemilatusRectum | 2e-12 | 0.0 | 2e-12

### Footnotes
(1) There is quite a large error in true longitude for this test. I am really not sure why given that `nyx` sums AoP, RAAN and TA to compute this, as per the definition. Summing these values leads _exactly_ to the value returned by `nyx`. I am very surprised that GMAT does not seem to use that same formula, I'll have to check why.

## From a Keplerian state

Element / Scenario  | circular inclined  | circular equatorial  | elliptical
--|---|---|--
Earth.X  | 0.0 | 0.0 | 0.0
Earth.Y  | 0.0 | 1e-14 | 4e-13
Earth.Z  | 0.0 | **5e-5**<sup>(1)</sup> | 3e-12
Earth.VX  | 0.0 | 1e-15 | 1e-15
Earth.VY  | 0.0 | 1e-8<sup>(2)</sup> | 2e-15
Earth.VZ  | 0.0 | 0.0 | 1e-15
Earth.Energy  | 0.0 | 0.0 | 1e-14
Earth.OrbitPeriod | 0.0 | 1e-11 | 5e-12
Earth.HX  | 0.0 | **2e-4**<sup>(3)</sup> | 0.0
Earth.HY  | 0.0 | **1e-3**<sup>(3)</sup> | 1e-11
Earth.HZ  | 0.0 | **0.0**<sup>(3)</sup> | 2e-11
Earth.SMA  | 0.0 | 1e-11 | 3e-12
Earth.ECC  | 0.0 | 2e-16 | 3e-16
EarthMJ2000Eq.INC | 3e-15 | 0.0 | 1e-14
EarthMJ2000Eq.RAAN | 0.0 | 0.0 | 0.0
EarthMJ2000Eq.AOP | 0.0 | 3e-8 | 1e-12
Earth.TA  | 0.0 | 0.0 | 1e-12
Earth.TLONG | 9e-14 | 0.0 | 1e-13
Earth.EA | 0.0 | 3e-8 | 1e-12
Earth.MA | 0.0 | 3e-8 | 1e-12
Earth.RadApo | 0.0 | 2e-11 | 1e-12
Earth.RadPer | 0.0 | 0.0 | 7e-12
Earth.SemilatusRectum | 1e-12 | 0.0 | 0.0

### Footnotes
(1) This is one heck of an oddity! The error is quite large, although the algorithm used here is strictly a conversion of GMAT's C++ code to Rust. I _suspect_ this error is due to the orbit being circular equatorial with a value of Z of **2.5 meters** according to `nyx` and **3.0 meters** according to GMAT. As noted above, `nyx` often returns one to two additional digits than GMAT does in its report files, so maybe does GMAT suffer from multiplying several rounding errors, or could it be some shortcuts that Rust/`nyx` make?

(2) Similarly to (1), we get a large error in the velocity Z component. In this case, GMAT reports 5.9e-8 km/s and `nyx` computes 4.9e-8 km/s.

(3) Similarly to (1), we get a very significant error in the orbital momentum computation of both HX and HY. These components are small for the orbital momentum (both on the order of 1e-3 in GMAT and in `nyx`). I am not too concerned about these differences given that the orbital momentum component of the Z axis is exactly that returned by GMAT (all 16 digits are equal).

# Multibody dynamics
The `multi_body_dynamics` test in tests/orbitaldyn.rs is the validation against GMAT. There seems to be a time difference between GMAT and TAI (and the RSS errors are minimized when adding an offset corresponding exactly to TT ... suspicious). **GMAT uses DE405 (released in May 1997) whereas `nyx` uses DE438s (released in March 2018): I believe this explains the large RSS error in position.**

In this test, nyx is 11 to 13 times faster than GMAT, while still running on a single core (like GMAT). The root mean squared errors in position and velocity are as follows:
```
RSS errors:     pos = 4.62544e-4 km     vel = 8.84538e-9 km/s
```

Final state  | x (km) | y (km) | z (km) | vx (km/s) | vy (km/s) | vz (km/s)
--|---|---|---|---|---|---|--
nyx  | 345350.664308004 | 5930.672405520 | 7333.283871226 | 0.02129819650 | 0.95667896503 | 0.30281758249 
GMAT  | 345350.664030479 | 5930.672047088 | 7333.283779286 | 0.02129819943 | 0.9566789568 | 0.3028175811
Abs. diff. | 2.775e-4 | 3.584e-4 | 9.194e-5 | 2.926e-9 | 8.231e-9 | 1.391e-9
