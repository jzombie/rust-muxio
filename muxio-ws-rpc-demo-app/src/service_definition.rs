use bitcode::{Decode, Encode};

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddRequestParams {
    pub numbers: Vec<f64>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AddResponseParams {
    pub result: f64,
}
