#[cfg(feature = "unvalidated")]
#[derive(Clone, Copy)]
pub struct Harmonics<S>
where
    S: GravityPotentialStor,
{
    neg_mu: f64,
    body_radius: f64,
    stor: S,
}

#[cfg(feature = "unvalidated")]
impl<S> Harmonics<S>
where
    S: GravityPotentialStor,
{
    /// Create a new Harmonics dynamical model from the provided gravity potential storage instance.
    pub fn from_stor<B: CelestialBody>(stor: S) -> Harmonics<S> {
        Harmonics {
            neg_mu: -B::gm(),
            body_radius: B::eq_radius(),
            stor,
        }
    }
}

#[cfg(feature = "unvalidated")]
impl<S: GravityPotentialStor> Dynamics for Harmonics<S> {
    type StateSize = U6;

    /// NOTE: No state is associated with Harmonics, always return zero time
    fn time(&self) -> f64 {
        0.0
    }

    /// NOTE: No state is associated with Harmonics, always return zero
    fn state(&self) -> VectorN<f64, Self::StateSize> {
        Vector6::zeros()
    }

    /// NOTE: Nothing happens in this `set_state` since there is no state of spherical harmonics.
    fn set_state(&mut self, _new_t: f64, _new_state: &VectorN<f64, Self::StateSize>) {}

    /// This provides a **DELTA** of the state, which must be added to the result of the TwoBody propagator being used.
    /// However, the provided `state` must be the position and velocity.
    fn eom(&self, _t: f64, state: &VectorN<f64, Self::StateSize>) -> VectorN<f64, Self::StateSize> {
        // NOTE: All this code is a conversion from GMAT's CalculateField1
        let radius = state.fixed_rows::<U3>(0).into_owned();
        // Using the GMAT notation, with extra character for ease of highlight
        let r_ = radius.norm();
        let s_ = radius[(0, 0)] / radius.norm();
        let t_ = radius[(1, 0)] / radius.norm();
        let u_ = radius[(2, 0)] / radius.norm();
        let max_degree = self.stor.max_degree() as usize; // In GMAT, the order is NN
        let max_order = self.stor.max_order() as usize; // In GMAT, the order is MM

        // Create the associated Legendre polynomials. Note that we add three items as per GMAT (this may be useful for the STM)
        let mut a_matrix: Vec<Vec<f64>> = (0..max_degree + 3)
            .map(|_| Vec::with_capacity(max_degree + 3))
            .collect();
        let mut vr01: Vec<Vec<f64>> = (0..max_degree + 3)
            .map(|_| Vec::with_capacity(max_degree + 3))
            .collect();
        let mut vr11: Vec<Vec<f64>> = (0..max_degree + 3)
            .map(|_| Vec::with_capacity(max_degree + 3))
            .collect();

        let mut re = Vec::with_capacity(max_degree + 3);
        let mut im = Vec::with_capacity(max_degree + 3);

        // Now that we have requested that capacity, let's set everything to zero so we can populate with [n][m].
        for n in 0..=max_degree + 2 {
            for _m in 0..=max_degree + 2 {
                a_matrix[n].push(0.0);
                vr01[n].push(0.0);
                vr11[n].push(0.0);
            }
            re.push(0.0);
            im.push(0.0);
        }

        let sqrt2 = 2.0f64.sqrt();

        for nu16 in 0..=max_degree {
            let n = nu16 as f64;
            for mu16 in 0..=min(nu16, max_order) {
                let m = mu16 as f64;
                vr01[nu16][mu16] = ((n - m) * (n + m + 1.0)).sqrt();
                vr11[nu16][mu16] =
                    (((2.0 * n + 1.0) * (n + m + 2.0) * (n + m + 1.0)) / (2.0 * n + 3.0)).sqrt();
                if mu16 == 0 {
                    vr01[nu16][mu16] /= sqrt2;
                    vr11[nu16][mu16] /= sqrt2;
                }
            }
        }

        // initialize the diagonal elements (not a function of the input)
        a_matrix[0][0] = 1.0; // Temp value for this first initialization
        for n in 1..=max_degree + 2 {
            let nf64 = n as f64;
            a_matrix[n][n] = ((2.0 * nf64 + 1.0) / (2.0 * nf64)).sqrt() * a_matrix[n - 1][n - 1]
        }

        a_matrix[1][0] = u_ * 3.0f64.sqrt();

        for nu16 in 1..=max_degree + 1 {
            let n = nu16 as f64;
            a_matrix[nu16 + 1][nu16] = u_ * ((2.0 * n + 3.0) as f64).sqrt() * a_matrix[nu16][nu16];
        }

        // apply column-fill recursion formula (Table 2, Row I, Ref.[1])
        for mu16 in 0..=max_order + 1 {
            let m = mu16 as f64;
            for nu16 in (mu16 + 2)..=max_degree + 1 {
                let n = nu16 as f64;
                let n1 = (((2.0 * n + 1.0) * (2.0 * n - 1.0)) / ((n - m) * (n + m))).sqrt();

                let n2 = (((2.0 * n + 1.0) * (n - m - 1.0) * (n + m - 1.0))
                    / ((2.0 * n - 3.0) * (n + m) * (n - m)))
                    .sqrt();

                a_matrix[nu16][mu16] =
                    u_ * n1 * a_matrix[nu16 - 1][mu16] - n2 * a_matrix[nu16 - 2][mu16];
            }
            // real part of (s + i*t)^m
            re[mu16] = if mu16 == 0 {
                1.0
            } else {
                s_ * re[(mu16 - 1)] - t_ * im[(mu16 - 1)]
            };
            im[mu16] = if mu16 == 0 {
                0.0
            } else {
                s_ * im[(mu16 - 1)] + t_ * re[(mu16 - 1)]
            }; // imaginary part of (s + i*t)^m
        }

        let rho = self.body_radius / r_;
        let mut rho_np1 = (-self.neg_mu / r_) * rho;
        let mut a1 = 0.0;
        let mut a2 = 0.0;
        let mut a3 = 0.0;
        let mut a4 = 0.0;

        for n in 1..=max_degree {
            rho_np1 *= rho;
            let mut sum1 = 0.0;
            let mut sum2 = 0.0;
            let mut sum3 = 0.0;
            let mut sum4 = 0.0;

            for m in 0..=min(n, max_order) {
                let (c_val, s_val) = self.stor.cs_nm(n as u16, m as u16);
                let d_ = (c_val * re[m] + s_val * im[m]) * sqrt2;
                let e_ = if m == 0 {
                    0.0
                } else {
                    (c_val * re[m - 1] + s_val * im[m - 1]) * sqrt2
                };
                let f_ = if m == 0 {
                    0.0
                } else {
                    (s_val * re[m - 1] - c_val * im[m - 1]) * sqrt2
                };

                sum2 += (m as f64) * a_matrix[n][m] * f_;
                sum1 += (m as f64) * a_matrix[n][m] * e_;
                sum3 += vr01[n][m] * a_matrix[n][m + 1] * d_;
                sum4 += vr11[n][m] * a_matrix[n + 1][m + 1] * d_;
            }
            let rr = rho_np1 / self.body_radius;
            a1 += rr * sum1;
            a2 += rr * sum2;
            a3 += rr * sum3;
            a4 -= rr * sum4;
        }
        Vector6::new(0.0, 0.0, 0.0, a1 + a4 * s_, a2 + a4 * t_, a3 + a4 * u_)
    }
}
