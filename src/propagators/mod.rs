extern crate nalgebra as na;

use self::error_ctrl::{ErrorCtrl, RSSStepPV};
use self::na::allocator::Allocator;
use self::na::{DefaultAllocator, VectorN};
use dynamics::Dynamics;
use std::f64;
use std::sync::mpsc::Sender;

/// Provides different methods for controlling the error computation of the integrator.
pub mod error_ctrl;

// Re-Export
mod rk;
pub use self::rk::*;
mod dormand;
pub use self::dormand::*;
mod fehlberg;
pub use self::fehlberg::*;
mod verner;
pub use self::verner::*;

/// The `RK` trait defines a Runge Kutta integrator.
pub trait RK
where
    Self: Sized,
{
    /// Returns the order of this integrator (as u8 because there probably isn't an order greater than 255).
    /// The order is used for the adaptive step size only to compute the error between estimates.
    fn order() -> u8;

    /// Returns the stages of this integrator (as usize because it's used as indexing)
    fn stages() -> usize;

    /// Returns a pointer to a list of f64 corresponding to the A coefficients of the Butcher table for that RK.
    /// This module only supports *implicit* integrators, and as such, `Self.a_coeffs().len()` must be of
    /// size (order+1)*(order)/2.
    /// *Warning:* this RK trait supposes that the implementation is consistent, i.e. c_i = \sum_j a_{ij}.
    fn a_coeffs() -> &'static [f64];
    /// Returns a pointer to a list of f64 corresponding to the b_i and b^*_i coefficients of the
    /// Butcher table for that RK. `Self.a_coeffs().len()` must be of size (order+1)*2.
    fn b_coeffs() -> &'static [f64];
}

/// Stores the details of the previous integration step of a given propagator. Access as `my_prop.clone().latest_details()`.
#[derive(Clone, Debug)]
pub struct IntegrationDetails {
    /// step size used
    pub step: f64,
    /// error in the previous integration step
    pub error: f64,
    /// number of attempts needed by an adaptive step size to be within the tolerance
    pub attempts: u8,
}

/// Includes the options, the integrator details of the previous step, and
/// the set of coefficients used for the monomorphic instance. **WARNING:** must be stored in a mutuable variable.
#[derive(Debug)]
pub struct Propagator<'a, M, E>
where
    M: Dynamics,
    E: ErrorCtrl,
    DefaultAllocator: Allocator<f64, M::StateSize>,
{
    pub dynamics: &'a mut M, // Stores the dynamics used. *Must* use this to get the latest values
    // An output channel for all of the states computed by this propagator
    pub tx_chan: Option<&'a Sender<(f64, VectorN<f64, M::StateSize>)>>,
    opts: PropOpts<E>,           // Stores the integration options (tolerance, min/max step, init step, etc.)
    details: IntegrationDetails, // Stores the details of the previous integration step
    step_size: f64,              // Stores the adapted step for the _next_ call
    order: u8,                   // Order of the integrator
    stages: usize,               // Number of stages, i.e. how many times the derivatives will be called
    a_coeffs: &'a [f64],
    b_coeffs: &'a [f64],
    fixed_step: bool,
}

/// The `Propagator` trait defines the functions of a propagator.
impl<'a, M: Dynamics, E: ErrorCtrl> Propagator<'a, M, E>
where
    DefaultAllocator: Allocator<f64, M::StateSize>,
{
    /// Each propagator must be initialized with `new` which stores propagator information.
    pub fn new<T: RK>(dynamics: &'a mut M, opts: &PropOpts<E>) -> Propagator<'a, M, E> {
        Propagator {
            tx_chan: None,
            dynamics,
            opts: *opts,
            details: IntegrationDetails {
                step: 0.0,
                error: 0.0,
                attempts: 1,
            },
            step_size: opts.init_step,
            stages: T::stages(),
            order: T::order(),
            a_coeffs: T::a_coeffs(),
            b_coeffs: T::b_coeffs(),
            fixed_step: T::stages() == usize::from(T::order()),
        }
    }

    pub fn set_fixed_step(&mut self, step_size: f64) {
        self.step_size = step_size;
        self.fixed_step = true;
    }

    /// Returns the time of the propagation
    ///
    /// WARNING: Do not use the dynamics to get the time, it will be the initial value!
    pub fn time(&self) -> f64 {
        self.dynamics.time()
    }

    /// Returns the state of the propagation
    ///
    /// WARNING: Do not use the dynamics to get the state, it will be the initial value!
    pub fn state(&self) -> VectorN<f64, M::StateSize> {
        self.dynamics.state()
    }

    /// This method propagates the provided Dynamics `dyn` for `elapsed_time` seconds. WARNING: This function has many caveats (please read detailed docs).
    ///
    /// ### IMPORTANT CAVEAT of `until_time_elapsed`
    /// - It is **assumed** that `self.dynamics.time()` returns a time in the same units as elapsed_time.
    pub fn until_time_elapsed(&mut self, elapsed_time: f64) -> (f64, VectorN<f64, M::StateSize>) {
        let backprop = elapsed_time < 0.0;
        if backprop {
            self.step_size *= -1.0; // Invert the step size
        }
        let init_seconds = self.dynamics.time();
        let stop_time = init_seconds + elapsed_time;
        loop {
            let dx = self.dynamics.state().clone();
            let dt = self.dynamics.time();
            let (t, state) = self.derive(dt, dx);
            if (t < stop_time && !backprop) || (t >= stop_time && backprop) {
                // We haven't passed the time based stopping condition.
                self.dynamics.set_state(t, &state.clone());
                if let Some(ref chan) = self.tx_chan {
                    if let Err(e) = chan.send((t, state.clone())) {
                        warn!("could not publish to channel: {}", e)
                    }
                }
            } else {
                let prev_details = self.latest_details().clone();
                let overshot = t - stop_time;
                if (!backprop && overshot > 0.0) || (backprop && overshot < 0.0) {
                    debug!("overshot by {} seconds", overshot);
                    self.set_fixed_step(prev_details.step - overshot);
                    // Take one final step
                    let dx = self.dynamics.state().clone();
                    let dt = self.dynamics.time();
                    let (t, state) = self.derive(dt, dx);
                    self.dynamics.set_state(t, &state.clone());
                    if let Some(ref chan) = self.tx_chan {
                        if let Err(e) = chan.send((t, state.clone())) {
                            warn!("could not publish to channel: {}", e)
                        }
                    }
                } else {
                    self.dynamics.set_state(t, &state.clone());
                    if let Some(ref chan) = self.tx_chan {
                        if let Err(e) = chan.send((t, state.clone())) {
                            warn!("could not publish to channel: {}", e)
                        }
                    }
                }
                return (t, state);
            }
        }
    }

    /// This method integrates whichever function is provided as `d_xdt`.
    ///
    /// The `derive` method is monomorphic to increase speed. This function takes a time `t` and a current state `state`
    /// then derives the dynamics at that time (i.e. propagates for one time step). The `d_xdt` parameter is the derivative
    /// function which take a time t of type f64 and a reference to a state of type VectorN<f64, N>, and returns the
    /// result as VectorN<f64, N> of the derivative. The reference should preferrably only be borrowed.
    /// This function returns the next time (i.e. the previous time incremented by the timestep used) and
    /// the new state as y_{n+1} = y_n + \frac{dy_n}{dt}. To get the integration details, check `Self.latest_details`.
    /// Note: using VectorN<f64, N> instead of DVector implies that the function *must* always return a vector of the same
    /// size. This static allocation allows for high execution speeds.
    pub fn derive(&mut self, t: f64, state: VectorN<f64, M::StateSize>) -> (f64, VectorN<f64, M::StateSize>) {
        // Reset the number of attempts used (we don't reset the error because it's set before it's read)
        self.details.attempts = 1;
        loop {
            let mut k = Vec::with_capacity(self.stages + 1); // Will store all the k_i.
            let ki = self.dynamics.eom(t, &state.clone());
            k.push(ki);
            let mut a_idx: usize = 0;
            for _ in 0..(self.stages - 1) {
                // Let's compute the c_i by summing the relevant items from the list of coefficients.
                // \sum_{j=1}^{i-1} a_ij  ∀ i ∈ [2, s]
                let mut ci: f64 = 0.0;
                // The wi stores the a_{s1} * k_1 + a_{s2} * k_2 + ... + a_{s, s-1} * k_{s-1} +
                let mut wi = VectorN::<f64, M::StateSize>::from_element(0.0);
                for kj in &k {
                    let a_ij = self.a_coeffs[a_idx];
                    ci += a_ij;
                    wi += a_ij * kj;
                    a_idx += 1;
                }

                let ki = self
                    .dynamics
                    .eom(t + ci * self.step_size, &(state.clone() + self.step_size * wi));
                k.push(ki);
            }
            // Compute the next state and the error
            let mut next_state = state.clone();
            // State error estimation from https://en.wikipedia.org/wiki/Runge%E2%80%93Kutta_methods#Adaptive_Runge%E2%80%93Kutta_methods
            // This is consistent with GMAT https://github.com/ChristopherRabotin/GMAT/blob/37201a6290e7f7b941bc98ee973a527a5857104b/src/base/propagator/RungeKutta.cpp#L537
            let mut error_est = VectorN::<f64, M::StateSize>::from_element(0.0);
            for (i, ki) in k.iter().enumerate() {
                let b_i = self.b_coeffs[i];
                if !self.fixed_step {
                    let b_i_star = self.b_coeffs[i + self.stages];
                    error_est += self.step_size * (b_i - b_i_star) * ki;
                }
                next_state += self.step_size * b_i * ki;
            }

            if self.fixed_step {
                // Using a fixed step, no adaptive step necessary
                self.details.step = self.step_size;
                return ((t + self.details.step), next_state);
            } else {
                // Compute the error estimate.
                self.details.error = E::estimate(&error_est, &next_state.clone(), &state.clone());
                if self.details.error <= self.opts.tolerance
                    || self.step_size <= self.opts.min_step
                    || self.details.attempts >= self.opts.attempts
                {
                    if self.details.attempts >= self.opts.attempts {
                        warn!("maximum number of attempts reached ({})", self.details.attempts);
                    }

                    self.details.step = self.step_size;
                    if self.details.error < self.opts.tolerance {
                        // Let's increase the step size for the next iteration.
                        // Error is less than tolerance, let's attempt to increase the step for the next iteration.
                        let proposed_step =
                            0.9 * self.step_size * (self.opts.tolerance / self.details.error).powf(1.0 / f64::from(self.order));
                        self.step_size = if proposed_step > self.opts.max_step {
                            self.opts.max_step
                        } else {
                            proposed_step
                        };
                    }
                    return ((t + self.details.step), next_state);
                } else {
                    // Error is too high and we aren't using the smallest step, and we haven't hit the max number of attempts.
                    // So let's adapt the step size.
                    self.details.attempts += 1;
                    let proposed_step =
                        0.9 * self.step_size * (self.opts.tolerance / self.details.error).powf(1.0 / f64::from(self.order - 1));
                    self.step_size = if proposed_step < self.opts.min_step {
                        self.opts.min_step
                    } else {
                        proposed_step
                    };
                }
            }
        }
    }

    /// Borrow the details of the latest integration step.
    pub fn latest_details(&self) -> &IntegrationDetails {
        &self.details
    }
}

/// PropOpts stores the integrator options, including the minimum and maximum step sizes, and the
/// max error size.
///
/// Note that different step sizes and max errors are only used for adaptive
/// methods. To use a fixed step integrator, initialize the options using `with_fixed_step`, and
/// use whichever adaptive step integrator is desired.  For example, initializing an RK45 with
/// fixed step options will lead to an RK4 being used instead of an RK45.
#[derive(Clone, Copy, Debug)]
pub struct PropOpts<E: ErrorCtrl> {
    init_step: f64,
    min_step: f64,
    max_step: f64,
    tolerance: f64,
    attempts: u8,
    fixed_step: bool,
    errctrl: E,
}

impl<E: ErrorCtrl> PropOpts<E> {
    /// `with_fixed_step` initializes an `PropOpts` such that the integrator is used with a fixed
    ///  step size.
    pub fn with_fixed_step(step: f64, errctrl: E) -> PropOpts<E> {
        PropOpts {
            init_step: step,
            min_step: step,
            max_step: step,
            tolerance: 0.0,
            fixed_step: true,
            attempts: 0,
            errctrl,
        }
    }

    /// `with_adaptive_step` initializes an `PropOpts` such that the integrator is used with an
    ///  adaptive step size. The number of attempts is currently fixed to 50 (as in GMAT).
    pub fn with_adaptive_step(min_step: f64, max_step: f64, tolerance: f64, errctrl: E) -> PropOpts<E> {
        PropOpts {
            init_step: max_step,
            min_step,
            max_step,
            tolerance,
            attempts: 50,
            fixed_step: false,
            errctrl,
        }
    }
}

impl Default for PropOpts<RSSStepPV> {
    /// `default` returns the same default options as GMAT.
    fn default() -> PropOpts<RSSStepPV> {
        PropOpts {
            init_step: 60.0,
            min_step: 0.001,
            max_step: 2700.0,
            tolerance: 1e-12,
            attempts: 50,
            fixed_step: false,
            errctrl: RSSStepPV {},
        }
    }
}

#[test]
fn test_options() {
    use self::error_ctrl::RSSStep;

    let opts = PropOpts::with_fixed_step(1e-1, RSSStep {});
    assert_eq!(opts.min_step, 1e-1);
    assert_eq!(opts.max_step, 1e-1);
    assert_eq!(opts.tolerance, 0.0);
    assert_eq!(opts.fixed_step, true);

    let opts = PropOpts::with_adaptive_step(1e-2, 10.0, 1e-12, RSSStep {});
    assert_eq!(opts.min_step, 1e-2);
    assert_eq!(opts.max_step, 10.0);
    assert_eq!(opts.tolerance, 1e-12);
    assert_eq!(opts.fixed_step, false);

    let opts: PropOpts<RSSStepPV> = Default::default();
    assert_eq!(opts.init_step, 60.0);
    assert_eq!(opts.min_step, 0.001);
    assert_eq!(opts.max_step, 2700.0);
    assert_eq!(opts.tolerance, 1e-12);
    assert_eq!(opts.attempts, 50);
    assert_eq!(opts.fixed_step, false);
}
