use arrow::array::FixedSizeListArray;
use sf_types::{ReadArrowError, ReadArrowType};

pub(super) fn vector_numbers(
    array: &FixedSizeListArray,
    row: usize,
) -> Result<Vec<f64>, ReadArrowError> {
    Ok(
        match sf_types::SnowflakeVector.read_arrow_type(array, row)? {
            sf_types::VectorCell::Int32(values) => values.iter().map(|&v| v as f64).collect(),
            // A direct f32-to-f64 conversion carries the 32-bit rounding
            // error along, handing JS 1.7999999523162842 for a stored 1.8.
            // Reparsing the shortest decimal that identifies the f32
            // ("1.8") gives the number JS means by 1.8.
            // TODO: the round trip through a decimal string is fragile and costs an
            //  allocation per element. Find a direct way to produce the JS value.
            sf_types::VectorCell::Float32(values) => values
                .iter()
                .copied()
                .map(widen_f32_via_shortest_decimal)
                .collect(),
        },
    )
}

fn widen_f32_via_shortest_decimal(value: f32) -> f64 {
    if value.is_finite() {
        value
            .to_string()
            .parse::<f64>()
            .unwrap_or_else(|_| unreachable!("f32 Display always parses as f64"))
    } else {
        f64::from(value)
    }
}
