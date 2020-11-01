use orthanc::*;
use regex::{Regex, RegexBuilder};
use std::env;
use std::process::Command;
use std::str;

const ORTHANC_ID_PATTERN: &str = r"(([0-9a-f]{8}-){4}[0-9a-f]{8})";
const ORTHANC_FAKE_ID: &str = "00000000-00000000-00000000-00000000-00000000";
const TRAILING_WHITESPACE_PATTERN: &str = r"([ ]+$)";
const VERSION_PATTERN: &str = r"orthanc \d+\.\d+\.\d+$";

const SOP_INSTANCE_UID: &str = "1.3.46.670589.11.1.5.0.3724.2011072815265975004";
const SOP_INSTANCE_UID_DELETE: &str = "1.3.46.670589.11.1.5.0.7080.2012100313435153441";
const SERIES_INSTANCE_UID: &str = "1.3.46.670589.11.1.5.0.3724.2011072815265926000";
const STUDY_INSTANCE_UID: &str = "1.3.46.670589.11.1.5.0.6560.2011072814060507000";
const PATIENT_ID: &str = "patient_2";

fn client() -> Client {
    Client::new(env::var("ORC_ORTHANC_ADDRESS").unwrap()).auth(
        env::var("ORC_ORTHANC_USERNAME").unwrap(),
        env::var("ORC_ORTHANC_PASSWORD").unwrap(),
    )
}

fn find_instance_by_sop_instance_uid(sop_instance_uid: &str) -> Option<Instance> {
    let instances = client().instances_expanded().unwrap();
    for i in instances {
        if i.main_dicom_tags["SOPInstanceUID"] == sop_instance_uid {
            return Some(i);
        }
    }
    return None;
}

fn find_series_by_series_instance_uid(series_instance_uid: &str) -> Option<Series> {
    let series = client().series_expanded().unwrap();
    for s in series {
        if s.main_dicom_tags["SeriesInstanceUID"] == series_instance_uid {
            return Some(s);
        }
    }
    return None;
}

fn find_study_by_study_instance_uid(study_instance_uid: &str) -> Option<Study> {
    let studies = client().studies_expanded().unwrap();
    for s in studies {
        if s.main_dicom_tags["StudyInstanceUID"] == study_instance_uid {
            return Some(s);
        }
    }
    return None;
}

fn find_patient_by_patient_id(patient_id: &str) -> Option<Patient> {
    let patients = client().patients_expanded().unwrap();
    for p in patients {
        if p.main_dicom_tags["PatientID"] == patient_id {
            return Some(p);
        }
    }
    return None;
}

fn fixup_output(text: &str) -> String {
    let orthanc_id_regex = Regex::new(ORTHANC_ID_PATTERN).unwrap();
    let trailing_whitespace_regex = RegexBuilder::new(TRAILING_WHITESPACE_PATTERN)
        .multi_line(true)
        .build()
        .unwrap();
    let version_regex = RegexBuilder::new(VERSION_PATTERN)
        .multi_line(true)
        .build()
        .unwrap();

    let no_orthanc_ids = orthanc_id_regex
        .replace_all(text, ORTHANC_FAKE_ID)
        .to_string();
    let no_trailing_whitespace = trailing_whitespace_regex
        .replace_all(&no_orthanc_ids, "")
        .to_string();
    version_regex
        .replace_all(&no_trailing_whitespace, "orthanc x.y.z")
        .to_string()
}

fn assert_result(
    args: Vec<&str>,
    expected_status: i32,
    expected_stdout: &str,
    expected_stderr: &str,
) {
    // Adapted from
    // https://github.com/assert-rs/assert_cmd/blob/d9fcca1ac40496afbcdaea719082e5d7f105f4d9/src/cargo.rs#L188
    let mut path = env::current_exe().unwrap();
    path.pop();
    path.pop();
    let res = Command::new(path.join("orthanc"))
        .args(&args)
        .output()
        .unwrap();
    assert_eq!(res.status.code().unwrap(), expected_status);
    assert_eq!(
        fixup_output(str::from_utf8(&res.stdout).unwrap()),
        expected_stdout
    );
    assert_eq!(
        fixup_output(str::from_utf8(&res.stderr).unwrap()),
        expected_stderr
    );
}

#[test]
fn test_help() {
    assert_result(vec![], 2, "", include_str!("data/help.stderr"));
}

#[test]
fn test_help_patient() {
    assert_result(
        vec!["patient"],
        2,
        "",
        include_str!("data/help_patient.stderr"),
    );
}

#[test]
fn test_help_study() {
    assert_result(vec!["study"], 2, "", include_str!("data/help_study.stderr"));
}

#[test]
fn test_help_series() {
    assert_result(
        vec!["series"],
        2,
        "",
        include_str!("data/help_series.stderr"),
    );
}

#[test]
fn test_help_instance() {
    assert_result(
        vec!["instance"],
        2,
        "",
        include_str!("data/help_instance.stderr"),
    );
}

//#[test]
fn test_list_patients() {
    assert_result(
        vec!["patient", "list"],
        0,
        include_str!("data/patient_list.stdout"),
        "",
    );
}

//#[test]
fn test_list_studies() {
    assert_result(
        vec!["study", "list"],
        0,
        include_str!("data/study_list.stdout"),
        "",
    );
}

//#[test]
fn test_list_series() {
    assert_result(
        vec!["series", "list"],
        0,
        include_str!("data/series_list.stdout"),
        "",
    );
}

//#[test]
fn test_list_instances() {
    assert_result(
        vec!["instance", "list"],
        0,
        include_str!("data/instance_list.stdout"),
        "",
    );
}

#[test]
fn test_show_patient() {
    let patient = find_patient_by_patient_id(PATIENT_ID).unwrap();
    assert_result(
        vec!["patient", "show", &patient.id],
        0,
        include_str!("data/patient_show.stdout"),
        "",
    );
}

#[test]
fn test_show_study() {
    let study = find_study_by_study_instance_uid(STUDY_INSTANCE_UID).unwrap();
    assert_result(
        vec!["study", "show", &study.id],
        0,
        include_str!("data/study_show.stdout"),
        "",
    );
}

#[test]
fn test_show_series() {
    let series = find_series_by_series_instance_uid(SERIES_INSTANCE_UID).unwrap();
    assert_result(
        vec!["series", "show", &series.id],
        0,
        include_str!("data/series_show.stdout"),
        "",
    );
}

#[test]
fn test_show_instance() {
    let instance = find_instance_by_sop_instance_uid(SOP_INSTANCE_UID).unwrap();
    assert_result(
        vec!["instance", "show", &instance.id],
        0,
        include_str!("data/instance_show.stdout"),
        "",
    );
}
