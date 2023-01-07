#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![warn(clippy::nursery)]
#![warn(clippy::str_to_string)]
#![warn(clippy::missing_inline_in_public_items)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::derive_partial_eq_without_eq)]

use clap::Parser;
use std::fmt;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
mod cli;
use cli::Args;

/// Store our verbosity level
static VERBOSITY: AtomicBool = AtomicBool::new(false);

/// Print to stderr if verbose mode is on
macro_rules! printvb {
    ($tt:tt) => {{
        if $crate::VERBOSITY.load(Ordering::Relaxed) {
            eprint!("[info]: ");
            eprintln!($tt);
        }
    }};
}

impl Args {
    /// Validate there are no redundant arguments. If so, print a message and return a code
    /// to exit.
    ///
    /// Also set `to_json`/`to_msgpack` based on file names if needed
    fn validate_update(&mut self) -> Result<(), Error> {
        VERBOSITY.store(self.verbose, Ordering::Relaxed);

        if self.to_json && self.to_msgpack {
            return Err(Error::ArgConflict(
                "--to-json".to_owned(),
                "--to-msgpack".to_owned(),
            ));
        }

        if self.input_file.is_some() && self.input.is_some() {
            return Err(Error::ArgConflict(
                "INPUT_FILE".to_owned(),
                "--text".to_owned(),
            ));
        }

        // Attempt to infer the input and output types
        if !self.to_json && !self.to_msgpack {
            let in_ty: FType = self
                .input_file
                .as_ref()
                .map_or(FType::None, FType::from_fname);
            let out_ty: FType = self
                .output_file
                .as_ref()
                .map_or(FType::None, FType::from_fname);
            let output_mp = matches!(in_ty, FType::Json) || matches!(out_ty, FType::MessagePack);
            let output_json = matches!(in_ty, FType::MessagePack) || matches!(out_ty, FType::Json);

            if output_mp {
                printvb!("inferring output type to be messagepack based on extension");
                self.to_msgpack = true;
            } else if output_json {
                printvb!("inferring output type to be json based on extension");
                self.to_json = true;
            } else {
                printvb!("file extensions not provided or do not match known types");
                return Err(Error::NoDirection);
            }
        }

        Ok(())
    }

    /// Get input from stdin or a file, as a string
    fn get_input<'a>(&'a self) -> Result<Box<dyn Read + 'a>, Error> {
        if let Some(fname) = self.input_file.as_ref() {
            printvb!("attempting to set input from file");
            let file = fs::File::open(fname).map_err(|e| Error::File(fname.clone(), e))?;
            Ok(Box::new(file))
        } else if let Some(txt) = self.input.as_ref() {
            Ok(Box::new(io::Cursor::new(txt)))
        } else {
            printvb!("getting input from stdin");
            Ok(Box::new(io::stdin()))
        }
    }

    /// Get the output location
    fn get_output(&self) -> Result<Box<dyn Write>, Error> {
        if let Some(fname) = self.output_file.as_ref() {
            printvb!("attempting to set output from file");

            let file = fs::File::create(fname).map_err(|e| Error::File(fname.clone(), e))?;
            Ok(Box::new(file))
        } else {
            printvb!("setting output to sdtout");

            Ok(Box::new(io::stdout()))
        }
    }
}

/// Representation of our exit codes
#[repr(u8)]
#[derive(Debug)]
#[allow(dead_code)]
pub enum Error {
    ArgConflict(String, String),
    NoDirection,
    NoInput,
    Hex(hex::FromHexError),
    // The string holds, the file name, io error is the error
    File(String, io::Error),
    Io(io::Error),
    MessagePak(rmp_serde::encode::Error),
    Json(serde_json::Error),
    // Generic error
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArgConflict(a1, a2) => write!(
                f,
                "Both {a1} and {a2} given, but only one is allowed at a time"
            ),
            Self::NoDirection => write!(
                f,
                "Could not determine conversion direction. Specify '--to-json' or '--to-msgpack'."
            ),
            Self::MessagePak(e) => write!(f, "Unable to parse MessagePack input, {e}"),
            Self::Json(e) => write!(f, "Unable to parse JSON input, {e}"),
            Self::Hex(e) => write!(f, "Invalid hex: {e}"),
            Self::NoInput => write!(f, "No input is present"),
            Self::File(n, e) => write!(f, "Error reading '{n}': {e}"),
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

/// Allow converting an Error to a `ExitCode` type
impl From<Error> for ExitCode {
    fn from(value: Error) -> Self {
        let tmp: u8 = match value {
            // Arg errors < 20
            Error::ArgConflict(_, _) => 4,
            Error::NoDirection => 6,
            // Parse/IO errors > 20
            Error::NoInput => 20,
            Error::Io(_) => 30,
            Error::File(_, _) => 31,
            Error::MessagePak(_) => 40,
            Error::Json(_) => 41,
            Error::Hex(_) => 42,
            Error::Other(_) => 90,
        };
        Self::from(tmp)
    }
}

/// Type of file we work with
enum FType {
    Json,
    MessagePack,
    None,
}

/// Returns whether a file name ends with a messagepack extension or a JSON
/// extension (in that order)
impl FType {
    fn from_fname<S: AsRef<str>>(fname: S) -> Self {
        let fpath = Path::new(fname.as_ref());
        let is_json = fpath
            .extension()
            .map_or(false, |ext| ext.eq_ignore_ascii_case("json"));

        if is_json {
            return Self::Json;
        }

        let is_mpk = fpath.extension().map_or(false, |ext| {
            ext.eq_ignore_ascii_case("msgpack") || ext.eq_ignore_ascii_case("mpk")
        });

        if is_mpk {
            Self::MessagePack
        } else {
            Self::None
        }
    }
}

/// Write messagepack from input to outpu
fn mp_to_json<R, W>(input: &mut R, output: &mut W, read_hex: bool) -> Result<(), Error>
where
    R: Read,
    W: Write,
{
    printvb!("transcoding from MessagePack to JSON");

    let mut inbuf = Vec::new();
    let byte_count = input.read_to_end(&mut inbuf).map_err(Error::Io)?;
    printvb!("read {byte_count} bytes");

    let hex_buf: Vec<u8>;

    // If we are requested to decode the hex input, do so
    let deser_buf = if read_hex {
        printvb!("decoding hex string input");
        inbuf.retain(|c| !c.is_ascii_whitespace());
        hex_buf = hex::decode(inbuf).map_err(Error::Hex)?;
        hex_buf.as_slice()
    } else {
        inbuf.as_slice()
    };

    let mut deserializer = rmp_serde::Deserializer::new(deser_buf);
    let mut serializer = serde_json::Serializer::new(output);
    serde_transcode::transcode(&mut deserializer, &mut serializer).map_err(Error::Json)?;

    // Return ownership of the output
    serializer
        .into_inner()
        .write_all(b"\n")
        .map_err(Error::Io)?;
    Ok(())
}

/// Write json to messagepack output. This is streaming
fn json_to_mp<R, W>(input: &mut R, output: &mut W, write_hex: bool) -> Result<(), Error>
where
    R: Read,
    W: Write,
{
    let mut deserializer = serde_json::Deserializer::from_reader(input);

    if write_hex {
        printvb!("using hex string output");
        let hex_buf = Vec::new();
        let mut serializer = rmp_serde::Serializer::new(io::Cursor::new(hex_buf));

        serde_transcode::transcode(&mut deserializer, &mut serializer)
            .map_err(Error::MessagePak)?;

        output
            .write_all(hex::encode(serializer.into_inner().into_inner()).as_bytes())
            .map_err(Error::Io)?;
        output.write_all(b"\n").map_err(Error::Io)?;
    } else {
        let mut serializer = rmp_serde::Serializer::new(output);
        serde_transcode::transcode(&mut deserializer, &mut serializer)
            .map_err(Error::MessagePak)?;
    };

    Ok(())
}

fn main_runner() -> Result<(), Error> {
    let mut args = Args::parse();

    // Validate arguments, return an error if applicable
    args.validate_update()?;
    let mut input = args.get_input()?;
    let mut output = args.get_output()?;

    if args.to_json {
        mp_to_json(&mut input, &mut output, args.hex)?;
    } else {
        json_to_mp(&mut input, &mut output, args.hex)?;
    }

    Ok(())
}

fn main() -> ExitCode {
    let res = main_runner();

    if let Err(e) = res {
        eprintln!("{e}");
        eprintln!("Command failed, exiting");
        e.into()
    } else {
        ExitCode::SUCCESS
    }
}
