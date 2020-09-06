use super::RPMPackageMetadata;
use crate::errors::RPMError;
use std::io;

/// Wraps multiple writers and destinations for efficient stream processing of RPM's
///
/// It is often necessary to parse RPM metadata for further processing. This makes it hard
/// to use stream based verifications like signature verifications. 
/// By using a `RPMProcessor` it is possible to first parse the package metadata and then proceed with processing.
///
/// # Examples
/// ```rust
/// use crate::RPMPackageMetadata;
/// use std::fs::File;
/// use std::io::prelude::*;
/// use std::io::BufReader;
///
/// let f = File::open("../../../test_assets/389-ds-base-devel-1.3.8.4-15.el7.x86_64.rpm").expect("unable to open dev rpm");
/// let buf_reader = BufReader::new(f);
/// let metadata = RPMPackageMetadata::parse(&mut buf_reader).expect("unable to parse rpm metadata");
/// let processor = RPMProcessor::new(metadata,buf_reader)
///                               .add_verifier()
///                               .add_destination()
///                               .process()
///                               .expect("unable to process rpm")
///
///
///
/// ```
pub struct RPMProcessor<'a, R: io::Read> {
    multi_writer: MultiWriter<'a>,
    body_input: R,
    metadata: &'a RPMPackageMetadata,
}

pub trait ProcessVerifier: io::Write {
    fn verify(&self, metadata: &RPMPackageMetadata) -> Result<(), RPMError>; // gets called when input is completely consumed
}

impl<'a, R: io::Read> RPMProcessor<'a, R> {
    pub fn new(metadata: &'a RPMPackageMetadata, body_input: R) -> Self {
        let multi_writer = MultiWriter {
            verifiers: Vec::new(),
            destinations: Vec::new(),
        };
        RPMProcessor {
            multi_writer,
            metadata,
            body_input,
        }
    }
    pub fn add_verifier<T: ProcessVerifier + 'a>(mut self, verifier: T) -> Self {
        self.multi_writer.verifiers.push(Box::new(verifier));
        self
    }

    pub fn add_destination<W: io::Write + 'a>(mut self, destination: W) -> Self {
        self.multi_writer.destinations.push(Box::new(destination));
        self
    }

    pub fn process(mut self) -> Result<(), RPMError> {
        self.metadata.write(&mut self.multi_writer)?;
        io::copy(&mut self.body_input, &mut self.multi_writer)?;
        self.multi_writer.verify(&self.metadata)?;
        Ok(())
    }
}


struct MultiWriter<'a> {
    destinations: Vec<Box<dyn io::Write + 'a>>,
    verifiers: Vec<Box<dyn ProcessVerifier + 'a>>,
}

impl<'a> ProcessVerifier for  MultiWriter<'a> {
    fn verify(&self, metadata: &RPMPackageMetadata) -> Result<(), RPMError> {
        for verifier in &self.verifiers {
            verifier.verify(metadata)?;
        }
        Ok(())
    }
}


impl <'a> io::Write for MultiWriter<'a> {

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for verifier in &mut self.verifiers {
            verifier.write(buf)?;
        }
        let mut  written = 0;
        for destination in &mut self.destinations {
            written = destination.write(buf)?;
        }
        return Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        for verifier in &mut self.verifiers {
            verifier.flush()?;
        }
        for destination in &mut self.destinations {
            destination.flush()?;
        }
        return Ok(())
    }
}
