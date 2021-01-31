extern crate csv;
extern crate hifitime;
extern crate nalgebra as na;
extern crate nyx_space as nyx;
extern crate pretty_env_logger;

use self::hifitime::{Epoch, SECONDS_PER_DAY};
use self::na::{Matrix2, Matrix6, Vector2, Vector6};
use self::nyx::celestia::{Cosm, Orbit};
use self::nyx::dynamics::orbital::{OrbitalDynamics, OrbitalDynamicsStm};
use self::nyx::dynamics::sph_harmonics::{Harmonics, HarmonicsDiff};
use self::nyx::io::gravity::*;
use self::nyx::od::ui::*;
use self::nyx::propagators::{PropOpts, Propagator, RK4Fixed};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

#[test]
fn ekf_fixed_step_perfect_stations() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }
    use std::thread;

    let cosm = Cosm::de438();

    // Define the ground stations.
    let ekf_num_meas = 100;
    // Set the disable time to be very low to test enable/disable sequence
    let ekf_disable_time = 5.0;
    let elevation_mask = 0.0;
    let range_noise = 0.0;
    let range_rate_noise = 0.0;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = SECONDS_PER_DAY;
    let step_size = 10.0;
    let opts = PropOpts::with_fixed_step(step_size);

    // Define the storages (channels for the states and a map for the measurements).
    let (truth_tx, truth_rx): (Sender<Orbit>, Receiver<Orbit>) = mpsc::channel();
    let mut measurements = Vec::with_capacity(10000); // Assume that we won't get more than 10k measurements.

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Generate the truth data on one thread.
    thread::spawn(move || {
        let mut dynamics = OrbitalDynamics::two_body(initial_state);
        let mut prop = Propagator::new::<RK4Fixed>(&mut dynamics, opts);
        prop.tx_chan = Some(truth_tx);
        prop.for_duration(prop_time).unwrap();
    });

    // Receive the states on the main thread, and populate the measurement channel.
    while let Ok(rx_state) = truth_rx.recv() {
        for station in all_stations.iter() {
            let meas = station.measure(&rx_state).unwrap();
            if meas.visible() {
                measurements.push(meas);
                break; // We know that only one station is in visibility at each time.
            }
        }
    }

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut tb_estimator = OrbitalDynamicsStm::two_body(initial_state);
    let prop_est = Propagator::new::<RK4Fixed>(&mut tb_estimator, opts);
    let covar_radius = 1.0e-6;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    // Define the initial estimate
    let initial_estimate = KfEstimate::from_covar(initial_state, init_covar);
    println!("initial estimate:\n{}", initial_estimate);

    // Define the expected measurement noise (we will then expect the residuals to be within those bounds if we have correctly set up the filter)
    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));

    let kf = KF::no_snc(initial_estimate, measurement_noise);

    let mut odp = ODProcess::ekf(
        prop_est,
        kf,
        all_stations,
        false,
        measurements.len(),
        StdEkfTrigger::new(ekf_num_meas, ekf_disable_time),
    );

    odp.process_measurements(&measurements).unwrap();

    // Check that the covariance deflated
    let est = &odp.estimates[odp.estimates.len() - 1];
    println!("{}", est.state());
    for i in 0..6 {
        assert!(
            est.covar[(i, i)] >= 0.0,
            "covar diagonal element negative @ [{}, {}]",
            i,
            i
        );
    }
    for i in 0..6 {
        if i < 3 {
            assert!(
                est.covar[(i, i)] < covar_radius,
                "covar radius did not decrease"
            );
        } else {
            assert!(
                est.covar[(i, i)] < covar_velocity,
                "covar velocity did not decrease"
            );
        }
    }
}

#[test]
fn ckf_fixed_step_perfect_stations() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }
    use std::{io, thread};

    let cosm = Cosm::de438();

    // Define the ground stations.
    let elevation_mask = 0.0;
    let range_noise = 0.0;
    let range_rate_noise = 0.0;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = SECONDS_PER_DAY;
    let step_size = 10.0;
    let opts = PropOpts::with_fixed_step(step_size);

    // Define the storages (channels for the states and a map for the measurements).
    let (truth_tx, truth_rx): (Sender<Orbit>, Receiver<Orbit>) = mpsc::channel();
    let mut measurements = Vec::with_capacity(10000); // Assume that we won't get more than 10k measurements.

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Generate the truth data on one thread.
    thread::spawn(move || {
        let mut dynamics = OrbitalDynamics::two_body(initial_state);
        let mut prop = Propagator::new::<RK4Fixed>(&mut dynamics, opts);
        prop.tx_chan = Some(truth_tx);
        prop.for_duration(prop_time).unwrap();
    });

    // Receive the states on the main thread, and populate the measurement channel.
    while let Ok(rx_state) = truth_rx.recv() {
        // Convert the state to ECI.
        for station in all_stations.iter() {
            let meas = station.measure(&rx_state).unwrap();
            if meas.visible() {
                measurements.push(meas);
                break; // We know that only one station is in visibility at each time.
            }
        }
    }

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut tb_estimator = OrbitalDynamicsStm::two_body(initial_state);
    let prop_est = Propagator::new::<RK4Fixed>(&mut tb_estimator, opts);
    let covar_radius = 1.0e-3;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    // Define the initial estimate
    let initial_estimate = KfEstimate::from_covar(initial_state, init_covar);

    // Define the expected measurement noise (we will then expect the residuals to be within those bounds if we have correctly set up the filter)
    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));

    let ckf = KF::no_snc(initial_estimate, measurement_noise);

    let mut odp = ODProcess::ckf(
        prop_est,
        ckf,
        all_stations.clone(),
        false,
        measurements.len(),
    );

    odp.process_measurements(&measurements).unwrap();

    let mut wtr = csv::Writer::from_writer(io::stdout());
    let mut printed = false;
    for (no, est) in odp.estimates.iter().enumerate() {
        if no == 0 {
            // Skip the first estimate which is the initial estimate provided by user
            continue;
        }
        assert!(
            est.state_deviation().norm() < 1e-6,
            "estimate error should be zero (perfect dynamics) ({:e})",
            est.state_deviation().norm()
        );

        if !printed {
            wtr.serialize(est.clone())
                .expect("could not write to stdout");
            printed = true;
        }
    }

    for res in &odp.residuals {
        assert!(
            res.postfit.norm() < 1e-12,
            "postfit should be zero (perfect dynamics) ({:e})",
            res
        );
    }

    // Check that the covariance deflated
    let estimates = odp.estimates.clone();
    let est = &estimates[estimates.len() - 1];
    for i in 0..6 {
        assert!(
            est.covar[(i, i)] >= 0.0,
            "covar diagonal element negative @ [{}, {}]",
            i,
            i
        );
    }
    for i in 0..6 {
        if i < 3 {
            assert!(
                est.covar[(i, i)] < covar_radius,
                "covar radius did not decrease"
            );
        } else {
            assert!(
                est.covar[(i, i)] < covar_velocity,
                "covar velocity did not decrease"
            );
        }
    }

    println!("N-1 not smoothed: \n{}", estimates[estimates.len() - 2]);

    // Iterate
    odp.iterate(&measurements, SmoothingArc::TimeGap(10.0))
        .unwrap();

    println!(
        "N-1 one iteration: \n{}",
        odp.estimates[odp.estimates.len() - 1]
    );

    println!(
        "Initial state after iteration: \n{:o}",
        odp.estimates[0].state()
    );
}

#[test]
fn ckf_fixed_step_iteration_test() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }
    use std::thread;

    let cosm = Cosm::de438();

    // Define the ground stations.
    let elevation_mask = 0.0;
    let range_noise = 0.1;
    let range_rate_noise = 0.001;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = SECONDS_PER_DAY;
    let step_size = 10.0;
    let opts = PropOpts::with_fixed_step(step_size);

    // Define the storages (channels for the states and a map for the measurements).
    let (truth_tx, truth_rx): (Sender<Orbit>, Receiver<Orbit>) = mpsc::channel();
    let mut measurements = Vec::with_capacity(10000); // Assume that we won't get more than 10k measurements.

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Generate the truth data on one thread.
    thread::spawn(move || {
        let mut dynamics = OrbitalDynamics::two_body(initial_state);
        let mut prop = Propagator::new::<RK4Fixed>(&mut dynamics, opts);
        prop.tx_chan = Some(truth_tx);
        prop.for_duration(prop_time).unwrap();
    });

    // Receive the states on the main thread, and populate the measurement channel.
    while let Ok(rx_state) = truth_rx.recv() {
        // Convert the state to ECI.
        for station in all_stations.iter() {
            let meas = station.measure(&rx_state).unwrap();
            if meas.visible() {
                measurements.push(meas);
                break; // We know that only one station is in visibility at each time.
            }
        }
    }

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut tb_estimator = OrbitalDynamicsStm::two_body(initial_state);
    let prop_est = Propagator::new::<RK4Fixed>(&mut tb_estimator, opts);
    let covar_radius = 1.0e-3;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    // Define the initial estimate
    let mut initial_state2 = initial_state;
    initial_state2.x += 0.1;
    initial_state2.y -= 0.1;
    initial_state2.z += 0.05;
    let initial_estimate = KfEstimate::from_covar(initial_state2, init_covar);

    // Define the expected measurement noise (we will then expect the residuals to be within those bounds if we have correctly set up the filter)
    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));

    let ckf = KF::no_snc(initial_estimate, measurement_noise);

    let mut odp = ODProcess::ckf(
        prop_est,
        ckf,
        all_stations.clone(),
        false,
        measurements.len(),
    );

    odp.process_measurements(&measurements).unwrap();

    // Iterate, and check that the initial state difference is lower
    // Iterate
    odp.iterate(&measurements, SmoothingArc::TimeGap(10.0))
        .unwrap();

    let dstate_no_iteration = initial_state - initial_state2;
    let dstate_iteration = initial_state - odp.estimates[0].state();

    println!("{}\n{}", initial_state2, odp.estimates[0].state());

    println!(
        "{}\n{}",
        dstate_no_iteration.rmag(),
        dstate_iteration.rmag()
    )
}

#[test]
fn ckf_fixed_step_perfect_stations_snc_covar_map() {
    // Tests state noise compensation with covariance mapping
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }
    use std::thread;

    let cosm = Cosm::de438();

    // Define the ground stations.
    let elevation_mask = 0.0;
    let range_noise = 0.0;
    let range_rate_noise = 0.0;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = SECONDS_PER_DAY;
    let step_size = 10.0;
    let opts = PropOpts::with_fixed_step(step_size);

    // Define the storages (channels for the states and a map for the measurements).
    let (truth_tx, truth_rx): (Sender<Orbit>, Receiver<Orbit>) = mpsc::channel();
    let mut measurements = Vec::with_capacity(10000); // Assume that we won't get more than 10k measurements.

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Generate the truth data on one thread.
    thread::spawn(move || {
        let mut dynamics = OrbitalDynamics::two_body(initial_state);
        let mut prop = Propagator::new::<RK4Fixed>(&mut dynamics, opts);
        prop.tx_chan = Some(truth_tx);
        prop.for_duration(prop_time).unwrap();
    });

    // Receive the states on the main thread, and populate the measurement channel.
    while let Ok(rx_state) = truth_rx.recv() {
        // Convert the state to ECI.
        for station in all_stations.iter() {
            let meas = station.measure(&rx_state).unwrap();
            if meas.visible() {
                measurements.push(meas);
                break; // We know that only one station is in visibility at each time.
            }
        }
    }

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut tb_estimator = OrbitalDynamicsStm::two_body(initial_state);
    let prop_est = Propagator::new::<RK4Fixed>(&mut tb_estimator, opts);

    // Set up the filter
    let covar_radius = 1.0e-3;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    // Define the initial estimate
    let initial_estimate = KfEstimate::from_covar(initial_state, init_covar);

    // Define the expected measurement noise (we will then expect the residuals to be within those bounds if we have correctly set up the filter)
    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));

    // Define the process noise to assume an unmodel acceleration of 1e-3 km^2/s^2 on X, Y and Z in the ECI frame
    let sigma_q = 1e-8_f64.powi(2);
    let process_noise = SNC3::from_diagonal(120.0, &[sigma_q, sigma_q, sigma_q]);

    let ckf = KF::new(initial_estimate, process_noise, measurement_noise);

    let mut odp = ODProcess::ckf(prop_est, ckf, all_stations, false, measurements.len());

    odp.process_measurements(&measurements).unwrap();

    let mut wtr = csv::Writer::from_path("./estimation.csv").unwrap();

    // Let's export these to a CSV file, and also check that the covariance never falls below our sigma squared values
    for (no, est) in odp.estimates.iter().enumerate() {
        if no == 1 {
            println!("{}", est);
        }
        assert!(
            est.state_deviation().norm() < 1e-6,
            "estimate error should be zero (perfect dynamics) ({:e})",
            est.state_deviation().norm()
        );

        for i in 0..6 {
            assert!(
                est.covar[(i, i)] >= 0.0,
                "covar diagonal element negative @ [{}, {}]",
                i,
                i
            );
        }

        for i in 0..6 {
            assert!(
                est.covar[(i, i)] >= sigma_q,
                "covar diagonal less than SNC value @ {} = {:.3e}",
                no,
                est.covar[(i, i)]
            );
        }

        wtr.serialize(est.clone())
            .expect("could not write to stdout");
    }
}

#[test]
fn ckf_map_covar() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }

    let cosm = Cosm::de438();

    // Define the ground stations.
    let elevation_mask = 0.0;
    let range_noise = 0.0;
    let range_rate_noise = 0.0;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = 2.0 * SECONDS_PER_DAY;
    let step_size = 10.0;

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut tb_estimator = OrbitalDynamicsStm::two_body(initial_state);

    let prop_est =
        Propagator::new::<RK4Fixed>(&mut tb_estimator, PropOpts::with_fixed_step(step_size));
    let covar_radius = 1.0e-3;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    let initial_estimate = KfEstimate::from_covar(initial_state, init_covar);

    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));
    let ckf = KF::no_snc(initial_estimate, measurement_noise);

    let mut odp = ODProcess::default_ckf(prop_est, ckf, all_stations);

    odp.map_covar(dt + prop_time).unwrap();

    // Check that the covariance inflated
    let estimates = odp.estimates;
    let est = &estimates[estimates.len() - 1];
    for i in 0..6 {
        assert!(
            est.covar[(i, i)] >= 0.0,
            "covar diagonal element negative @ [{}, {}]",
            i,
            i
        );
    }
    for i in 0..6 {
        if i < 3 {
            assert!(
                est.covar[(i, i)] > covar_radius,
                "covar radius did not increase"
            );
        } else {
            assert!(
                est.covar[(i, i)] > covar_velocity,
                "covar velocity did not increase"
            );
        }
    }
}

#[test]
fn ckf_fixed_step_perfect_stations_harmonics() {
    // Tests state noise compensation with covariance mapping
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }

    use std::thread;

    let cosm = Cosm::de438();

    // Define the ground stations.
    let elevation_mask = 0.0;
    let range_noise = 0.0;
    let range_rate_noise = 0.0;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = SECONDS_PER_DAY;
    let step_size = 10.0;
    let opts = PropOpts::with_fixed_step(step_size);

    // Define the storages (channels for the states and a map for the measurements).
    let (truth_tx, truth_rx): (Sender<Orbit>, Receiver<Orbit>) = mpsc::channel();
    let mut measurements = Vec::with_capacity(10000); // Assume that we won't get more than 10k measurements.

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let iau_earth = cosm.frame("IAU Earth");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Generate the truth data on one thread.
    thread::spawn(move || {
        let cosm = Cosm::de438();
        let mut dynamics = OrbitalDynamics::two_body(initial_state);
        let earth_sph_harm = HarmonicsMem::from_cof("data/JGM3.cof.gz", 70, 70, true).unwrap();
        let harmonics = Harmonics::from_stor(iau_earth, earth_sph_harm, &cosm);
        dynamics.add_model(Box::new(harmonics));
        let mut prop = Propagator::new::<RK4Fixed>(&mut dynamics, opts);
        prop.tx_chan = Some(truth_tx);
        prop.for_duration(prop_time).unwrap();
    });

    // Receive the states on the main thread, and populate the measurement channel.
    while let Ok(rx_state) = truth_rx.recv() {
        // Convert the state to ECI.
        for station in all_stations.iter() {
            let meas = station.measure(&rx_state).unwrap();
            if meas.visible() {
                measurements.push(meas);
                break; // We know that only one station is in visibility at each time.
            }
        }
    }

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut estimator = OrbitalDynamicsStm::two_body(initial_state);
    let earth_sph_harm = HarmonicsMem::from_cof("data/JGM3.cof.gz", 70, 70, true).unwrap();
    let harmonics = HarmonicsDiff::from_stor(iau_earth, earth_sph_harm, &cosm);
    estimator.add_model(Box::new(harmonics));
    let prop_est = Propagator::new::<RK4Fixed>(&mut estimator, opts);

    // Set up the filter
    let covar_radius = 1.0e-3;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    // Define the initial estimate
    let initial_estimate = KfEstimate::from_covar(initial_state, init_covar);

    // Define the expected measurement noise (we will then expect the residuals to be within those bounds if we have correctly set up the filter)
    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));

    let ckf = KF::no_snc(initial_estimate, measurement_noise);

    let mut odp = ODProcess::ckf(prop_est, ckf, all_stations, false, measurements.len());

    odp.process_measurements(&measurements).unwrap();
    let mut wtr = csv::Writer::from_path("./estimation.csv").unwrap();

    // Let's export these to a CSV file, and also check that the covariance never falls below our sigma squared values
    for (no, est) in odp.estimates.iter().enumerate() {
        if no == 1 {
            println!("{}", est);
        }
        for i in 0..6 {
            assert!(
                est.covar[(i, i)] >= 0.0,
                "covar diagonal element negative @ [{}, {}]",
                i,
                i
            );
        }
        assert!(
            est.state_deviation().norm() < 1e-12,
            "estimate error should be zero (perfect dynamics) ({:e})",
            est.state_deviation().norm()
        );

        wtr.serialize(est.clone())
            .expect("could not write to stdout");
    }
}

#[test]
fn ckf_fixed_step_perfect_stations_several_snc_covar_map() {
    // Tests state noise compensation with covariance mapping
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }
    use std::thread;

    let cosm = Cosm::de438();

    // Define the ground stations.
    let elevation_mask = 0.0;
    let range_noise = 0.0;
    let range_rate_noise = 0.0;
    let dss65_madrid =
        GroundStation::dss65_madrid(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss34_canberra =
        GroundStation::dss34_canberra(elevation_mask, range_noise, range_rate_noise, &cosm);
    let dss13_goldstone =
        GroundStation::dss13_goldstone(elevation_mask, range_noise, range_rate_noise, &cosm);
    let all_stations = vec![dss65_madrid, dss34_canberra, dss13_goldstone];

    // Define the propagator information.
    let prop_time = SECONDS_PER_DAY;
    let step_size = 10.0;
    let opts = PropOpts::with_fixed_step(step_size);

    // Define the storages (channels for the states and a map for the measurements).
    let (truth_tx, truth_rx): (Sender<Orbit>, Receiver<Orbit>) = mpsc::channel();
    let mut measurements = Vec::with_capacity(10000); // Assume that we won't get more than 10k measurements.

    // Define state information.
    let eme2k = cosm.frame("EME2000");
    let dt = Epoch::from_mjd_tai(21545.0);
    let initial_state = Orbit::keplerian(22000.0, 0.01, 30.0, 80.0, 40.0, 0.0, dt, eme2k);

    // Generate the truth data on one thread.
    thread::spawn(move || {
        let mut dynamics = OrbitalDynamics::two_body(initial_state);
        let mut prop = Propagator::new::<RK4Fixed>(&mut dynamics, opts);
        prop.tx_chan = Some(truth_tx);
        prop.for_duration(prop_time).unwrap();
    });

    // Receive the states on the main thread, and populate the measurement channel.
    while let Ok(rx_state) = truth_rx.recv() {
        // Convert the state to ECI.
        for station in all_stations.iter() {
            let meas = station.measure(&rx_state).unwrap();
            if meas.visible() {
                measurements.push(meas);
                break; // We know that only one station is in visibility at each time.
            }
        }
    }

    // Now that we have the truth data, let's start an OD with no noise at all and compute the estimates.
    // We expect the estimated orbit to be perfect since we're using strictly the same dynamics, no noise on
    // the measurements, and the same time step.
    let mut tb_estimator = OrbitalDynamicsStm::two_body(initial_state);
    let prop_est = Propagator::new::<RK4Fixed>(&mut tb_estimator, opts);

    // Set up the filter
    let covar_radius = 1.0e-3;
    let covar_velocity = 1.0e-6;
    let init_covar = Matrix6::from_diagonal(&Vector6::new(
        covar_radius,
        covar_radius,
        covar_radius,
        covar_velocity,
        covar_velocity,
        covar_velocity,
    ));

    // Define the initial estimate
    let initial_estimate = KfEstimate::from_covar(initial_state, init_covar);

    // Define the expected measurement noise (we will then expect the residuals to be within those bounds if we have correctly set up the filter)
    let measurement_noise = Matrix2::from_diagonal(&Vector2::new(1e-6, 1e-3));

    // Define the process noise to assume an unmodel acceleration of 1e-3 km^2/s^2 on X, Y and Z in the ECI frame
    let sigma_q1 = 1e-7_f64.powi(2);
    let process_noise1 = SNC3::from_diagonal(120.0, &[sigma_q1, sigma_q1, sigma_q1]);

    let sigma_q2 = 1e-8_f64.powi(2);
    let sigma_q2_d = 3600.0;
    let mut process_noise2 = SNC3::with_decay(
        120.0,
        &[sigma_q2, sigma_q2, sigma_q2],
        &[sigma_q2_d, sigma_q2_d, sigma_q2_d],
    );
    process_noise2.start_time = Some(dt + 36_000.0); // Start the second process noise 10 hours into the tracking pass

    let ckf = KF::with_sncs(
        initial_estimate,
        vec![process_noise1, process_noise2],
        measurement_noise,
    );

    let mut odp = ODProcess::ckf(prop_est, ckf, all_stations, false, measurements.len());

    odp.process_measurements(&measurements).unwrap();

    let mut wtr = csv::Writer::from_path("./estimation.csv").unwrap();

    // Let's export these to a CSV file, and also check that the covariance never falls below our sigma squared values
    for (no, est) in odp.estimates.iter().enumerate() {
        if no == 1 {
            println!("{}", est);
        }
        assert!(
            est.state_deviation().norm() < 1e-6,
            "estimate error should be zero (perfect dynamics) ({:e})",
            est.state_deviation().norm()
        );

        for i in 0..6 {
            assert!(
                est.covar[(i, i)] >= 0.0,
                "covar diagonal element negative @ [{}, {}]",
                i,
                i
            );
        }

        wtr.serialize(est.clone())
            .expect("could not write to stdout");
    }
}
