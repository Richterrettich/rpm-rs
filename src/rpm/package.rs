use sha2::Digest;

use super::headers::*;

use crate::constants::*;
use crate::sequential_cursor::SeqCursor;

use crate::errors::*;

use super::Lead;
use crate::signature;

use std::io::{Read, Seek, SeekFrom};
/// A complete rpm file.
///
/// Can either be created using the [`RPMPackageBuilder`](super::builder::RPMPackageBuilder)
/// or used with [`parse`](`self::RPMPackage::parse`) to obtain from a file.
#[derive(Debug)]
pub struct RPMPackage {
    /// Header and metadata structures.
    ///
    /// Contains the constant lead as well as the metadata store.
    pub metadata: RPMPackageMetadata,
    /// The compressed or uncompressed files.
    pub content: Vec<u8>,
}

impl RPMPackage {
    pub fn parse<T: std::io::BufRead>(input: &mut T) -> Result<Self, RPMError> {
        let metadata = RPMPackageMetadata::parse(input)?;
        let mut content = Vec::new();
        input.read_to_end(&mut content)?;
        Ok(RPMPackage { metadata, content })
    }

    pub fn write<W: std::io::Write>(&self, out: &mut W) -> Result<(), RPMError> {
        self.metadata.write(out)?;
        out.write_all(&self.content)?;
        Ok(())
    }

    // TODO allow passing an external signer/verifier

    /// sign all headers (except for the lead) using an external key and store it as the initial header
    #[cfg(feature = "signature-meta")]
    pub fn sign<S>(&mut self, signer: S) -> Result<(), RPMError>
    where
        S: signature::Signing<signature::algorithm::RSA, Signature = Vec<u8>>,
    {
        // create a temporary byte repr of the header
        // and re-create all hashes
        let mut header_bytes = Vec::<u8>::with_capacity(1024);
        self.metadata.header.write(&mut header_bytes)?;

        let mut header_and_content_cursor =
            SeqCursor::new(&[header_bytes.as_slice(), self.content.as_slice()]);

        let mut hasher = md5::Md5::default();
        {
            // avoid loading it into memory all at once
            // since the content could be multiple 100s of MBs
            let mut buf = [0u8; 256];
            while let Ok(n) = header_and_content_cursor.read(&mut buf[..]) {
                hasher.update(&buf[0..n]);
            }
        }
        let hash_result = hasher.finalize();

        header_and_content_cursor.seek(SeekFrom::Start(0))?;

        let digest_md5 = hash_result.as_slice();

        let digest_sha1 = sha1::Sha1::from(&header_bytes);
        let digest_sha1 = digest_sha1.digest();

        let rsa_signature_spanning_header_only = signer.sign(header_bytes.as_slice())?;

        let rsa_signature_spanning_header_and_archive =
            signer.sign(&mut header_and_content_cursor)?;

        // TODO FIXME verify this is the size we want, I don't think it is
        // TODO maybe use signature_size instead of size
        self.metadata.signature = Header::<IndexSignatureTag>::new_signature_header(
            header_and_content_cursor.len() as i32,
            digest_md5,
            digest_sha1.to_string(),
            rsa_signature_spanning_header_only.as_slice(),
            rsa_signature_spanning_header_and_archive.as_slice(),
        );

        Ok(())
    }

    /// Verify the signature as present within the RPM package.
    ///
    ///
    #[cfg(feature = "signature-meta")]
    pub fn verify_signature<V>(&self, verifier: V) -> Result<(), RPMError>
    where
        V: signature::Verifying<signature::algorithm::RSA, Signature = Vec<u8>>,
    {
        // TODO retval should be SIGNATURE_VERIFIED or MISMATCH, not just an error

        let mut header_bytes = Vec::<u8>::with_capacity(1024);
        self.metadata.header.write(&mut header_bytes)?;

        let signature_header_only = self
            .metadata
            .signature
            .get_entry_binary_data(IndexSignatureTag::RPMSIGTAG_RSA)?;

        crate::signature::echo_signature("signature_header(header only)", signature_header_only);

        let signature_header_and_content = self
            .metadata
            .signature
            .get_entry_binary_data(IndexSignatureTag::RPMSIGTAG_PGP)?;

        crate::signature::echo_signature(
            "signature_header(header and content)",
            signature_header_and_content,
        );

        verifier.verify(header_bytes.as_slice(), signature_header_only)?;

        let header_and_content_cursor =
            SeqCursor::new(&[header_bytes.as_slice(), self.content.as_slice()]);

        verifier.verify(header_and_content_cursor, signature_header_and_content)?;

        Ok(())
    }
}

#[derive(PartialEq, Debug)]
pub struct RPMPackageMetadata {
    pub lead: Lead,
    pub signature: Header<IndexSignatureTag>,
    pub header: Header<IndexTag>,
}

impl RPMPackageMetadata {
    pub(crate) fn parse<T: std::io::BufRead>(input: &mut T) -> Result<Self, RPMError> {
        let mut lead_buffer = [0; LEAD_SIZE];
        input.read_exact(&mut lead_buffer)?;
        let lead = Lead::parse(&lead_buffer)?;
        let signature_header = Header::parse_signature(input)?;
        let header = Header::parse(input)?;
        Ok(RPMPackageMetadata {
            lead,
            signature: signature_header,
            header,
        })
    }

    pub(crate) fn write<W: std::io::Write>(&self, out: &mut W) -> Result<(), RPMError> {
        self.lead.write(out)?;
        self.signature.write_signature(out)?;
        self.header.write(out)?;
        Ok(())
    }
}
