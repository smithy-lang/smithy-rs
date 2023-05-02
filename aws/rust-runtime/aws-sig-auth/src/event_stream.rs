/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::Credentials;
use aws_sigv4::event_stream::{sign_empty_message, sign_message};
use aws_sigv4::SigningParams;
use aws_smithy_eventstream::frame::{Message, SignMessage, SignMessageError};
use aws_types::region::SigningRegion;
use aws_types::SigningService;
use std::time::SystemTime;

/// Event Stream SigV4 signing implementation.
#[derive(Debug)]
pub struct SigV4Signer {
    last_signature: String,
    credentials: Credentials,
    signing_region: SigningRegion,
    signing_service: SigningService,
    time: Option<SystemTime>,
}

impl SigV4Signer {
    pub fn new(
        last_signature: String,
        credentials: Credentials,
        signing_region: SigningRegion,
        signing_service: SigningService,
        time: Option<SystemTime>,
    ) -> Self {
        Self {
            last_signature,
            credentials,
            signing_region,
            signing_service,
            time,
        }
    }

    fn signing_params(&self) -> SigningParams<()> {
        let mut builder = SigningParams::builder()
            .access_key(self.credentials.access_key_id())
            .secret_key(self.credentials.secret_access_key())
            .region(self.signing_region.as_ref())
            .service_name(self.signing_service.as_ref())
            .time(self.time.unwrap_or_else(SystemTime::now))
            .settings(());
        builder.set_security_token(self.credentials.session_token());
        builder.build().unwrap()
    }
}

impl SignMessage for SigV4Signer {
    fn sign(&mut self, message: Message) -> Result<Message, SignMessageError> {
        let (signed_message, signature) = {
            let params = self.signing_params();
            sign_message(&message, &self.last_signature, &params).into_parts()
        };
        self.last_signature = signature;
        Ok(signed_message)
    }

    fn sign_empty(&mut self) -> Option<Result<Message, SignMessageError>> {
        let (signed_message, signature) = {
            let params = self.signing_params();
            sign_empty_message(&self.last_signature, &params).into_parts()
        };
        self.last_signature = signature;
        Some(Ok(signed_message))
    }
}

#[cfg(test)]
mod tests {
    use crate::event_stream::SigV4Signer;
    use aws_credential_types::Credentials;
    use aws_smithy_eventstream::frame::{HeaderValue, Message, SignMessage};
    use aws_types::region::Region;
    use aws_types::region::SigningRegion;
    use aws_types::SigningService;
    use std::time::{Duration, UNIX_EPOCH};

    fn check_send_sync<T: Send + Sync>(value: T) -> T {
        value
    }

    #[test]
    fn sign_message() {
        let region = Region::new("us-east-1");
        let mut signer = check_send_sync(SigV4Signer::new(
            "initial-signature".into(),
            Credentials::for_tests(),
            SigningRegion::from(region),
            SigningService::from_static("transcribe"),
            Some(UNIX_EPOCH + Duration::new(1611160427, 0)),
        ));
        let mut signatures = Vec::new();
        for _ in 0..5 {
            let signed = signer
                .sign(Message::new(&b"identical message"[..]))
                .unwrap();
            if let HeaderValue::ByteArray(signature) = signed
                .headers()
                .iter()
                .find(|h| h.name().as_str() == ":chunk-signature")
                .unwrap()
                .value()
            {
                signatures.push(signature.clone());
            } else {
                panic!("failed to get the :chunk-signature")
            }
        }
        for i in 1..signatures.len() {
            assert_ne!(signatures[i - 1], signatures[i]);
        }
    }
}
