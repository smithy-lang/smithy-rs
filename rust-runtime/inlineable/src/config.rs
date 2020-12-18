/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */


// TODO: Generate the config dynamically based on the requirements of the service
use std::sync::Mutex;

pub(crate) fn v4(input: u128) -> String {
    let mut out = String::with_capacity(36);
    // u4-aligned index into [input]
    let mut rnd_idx: u8 = 0;
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    for str_idx in 0..36 {
        if str_idx == 8 || str_idx == 13 || str_idx == 18 || str_idx == 23 {
            out.push('-');
            // UUID version character
        } else if str_idx == 14 {
            out.push('4');
        } else {
            let mut dat: u8 = ((input >> (rnd_idx * 4)) & 0x0F) as u8;
            // UUID variant bits
            if str_idx == 19 {
                dat |= 0b00001000;
            }
            rnd_idx += 1;
            out.push(HEX_CHARS[dat as usize] as char);
        }
    }
    out
}

pub trait ProvideIdempotencyToken {
    fn token(&self) -> String;
}

fn default_provider() -> impl ProvideIdempotencyToken {
    Mutex::new(rand::thread_rng())
}


impl<T> ProvideIdempotencyToken for Mutex<T> where T: rand::Rng {
    fn token(&self) -> String {
        let input: u128 = self.lock().unwrap().gen();
        v4(input)
    }
}

impl ProvideIdempotencyToken for &'static str {
    fn token(&self) -> String {
        self.to_string()
    }
}

pub struct Config {
    #[allow(dead_code)]
    pub(crate) token_provider: Box<dyn ProvideIdempotencyToken>
}

impl Config {
    pub fn from_env() -> Self {
        ConfigBuilder::new().build()
    }
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}

#[derive(Default)]
pub struct ConfigBuilder {
    #[allow(dead_code)]
    token_provider: Option<Box<dyn ProvideIdempotencyToken>>
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn token_provider(mut self, token_provider: impl ProvideIdempotencyToken + 'static) -> Self {
        self.token_provider = Some(Box::new(token_provider));
        self
    }

    pub fn build(self) -> Config {
        Config {
            token_provider: self.token_provider.unwrap_or_else(|| Box::new(default_provider()))
        }
    }
}
