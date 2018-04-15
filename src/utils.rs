extern crate nalgebra as na;
use self::na::{Matrix3, Vector3};
use std::f64;

/// Returns the tilde matrix from the provided Vector3.
pub fn tilde_matrix(v: &Vector3<f64>) -> Matrix3<f64> {
    Matrix3::new(
        0.0,
        -v[(2, 0)],
        v[(1, 0)],
        v[(2, 0)],
        0.0,
        -v[(0, 0)],
        -v[(1, 0)],
        v[(0, 0)],
        0.0,
    )
}

/// Returns whether the provided square matrix (3x3) is diagonal
pub fn is_diagonal(m: &Matrix3<f64>) -> bool {
    let mut is_diag = true;
    for i in 1..2 {
        for j in 0..i {
            if (i == j && (m[(i, j)] - m[(0, 0)]) > f64::EPSILON)
                || (i != j
                    && (m[(i, j)].abs() > f64::EPSILON
                        || (m[(i, j)] - m[(j, i)]).abs() > f64::EPSILON))
            {
                is_diag = false;
                break;
            }
        }
    }
    is_diag
}
