use crate::constants::*;
use crate::{CliError, Result};
use comfy_table::{ColumnConstraint, ContentArrangement, Table};
use orthanc::entity::*;
use orthanc::models::*;
use serde_yaml;
use std::collections::HashMap;
use std::{env, fs, process, result};

pub fn create_table(header: Option<&[&str]>) -> Table {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.load_preset(TABLE_PRESET);
    match header {
        Some(h) => table.set_header(h.iter()),
        None => &table,
    };
    table
}

pub fn create_list_table<T: Entity>(
    entities: Vec<T>,
    columns: &[&str],
    dicom_tags: &[&str],
    no_header: bool,
) -> Table {
    let header = if no_header { None } else { Some(columns) };
    let mut table = create_table(header);
    for entity in entities {
        let mut row: Vec<String> = vec![];

        if columns.contains(&"ID") {
            row.push(entity.id().to_string());
        };

        for t in dicom_tags.iter() {
            let val = entity
                .main_dicom_tag(t)
                .unwrap_or(ABSENT_DICOM_TAG_PLACEHOLDER);
            row.push(val.to_string());
        }

        match T::kind() {
            EntityKind::Instance => {
                if columns.contains(&"Index in series") {
                    let index_in_series = match entity.index() {
                        Some(i) => format!("{}", i),
                        None => "".to_string(),
                    };
                    row.push(index_in_series);
                };
                if columns.contains(&"File size") {
                    let file_size = format!("{}", entity.size());
                    row.push(file_size);
                };
            }
            _ => {
                if columns.contains(&"Number of Studies")
                    || columns.contains(&"Number of Series")
                    || columns.contains(&"Number of Instances")
                {
                    let num_children = format!("{}", entity.children_len());
                    row.push(num_children);
                }
            }
        }
        table.add_row(row.iter());
    }

    // This assumes the ID is always the first column
    if columns.contains(&"ID") {
        if let Some(id_column) = table.get_column_mut(0) {
            id_column.set_constraint(ColumnConstraint::MinWidth(ID_COLUMN_WIDTH));
        }
    };
    table
}

pub fn create_show_table<T: Entity>(entity: T, dicom_tags: &[&str]) -> Table {
    let mut table = create_table(None);
    table.add_row(["ID", entity.id()].iter());
    if T::kind() != EntityKind::Patient {
        table.add_row(
            [
                &format!("{} ID", entity.parent_kind_name().unwrap()),
                entity.parent_id().unwrap(),
            ]
            .iter(),
        );
    }

    for t in dicom_tags.iter() {
        table.add_row(
            [
                t,
                entity
                    .main_dicom_tag(t)
                    .unwrap_or(ABSENT_DICOM_TAG_PLACEHOLDER),
            ]
            .iter(),
        );
    }
    match T::kind() {
        EntityKind::Instance => {
            let index_in_series = match entity.index() {
                Some(i) => format!("{}", i),
                None => "".to_string(),
            };
            let file_size = format!("{}", entity.size());
            table.add_row(["Index in series", &index_in_series].iter());
            table.add_row(["File size", &file_size].iter());
        }
        _ => {
            let num_children = format!("{}", entity.children_len());
            table.add_row(
                [
                    &format!("Number of {}", entity.children_kind_name().unwrap()),
                    &num_children,
                ]
                .iter(),
            );
        }
    }
    table
}

pub fn create_new_entity_table(result: ModificationResult) -> Table {
    let mut table = create_table(None);
    table.add_row([format!("New {:?} ID", result.entity), result.id].iter());
    match result.entity {
        EntityKind::Patient => &table,
        _ => table.add_row(["Patient ID", &result.patient_id].iter()),
    };
    table
}

pub fn create_error_table(error: CliError) -> Table {
    let mut table = create_table(None);
    table.add_row(["Error", &error.error].iter());
    match error.message {
        Some(m) => {
            table.add_row(["Message", &m].iter());
        }
        None => (),
    };
    match error.details {
        Some(d) => {
            table.add_row(["Details", &d].iter());
        }
        None => (),
    };
    table
}

/// Parses the command-line option value(s) TagName=TagValue into a HashMap
pub fn parse_tag_kv_pairs(cmd_values: Vec<&str>) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for v in cmd_values {
        let tag_kv_pair: Vec<&str> = v.split("=").collect();
        if tag_kv_pair.len() != 2 {
            return Err(CliError::new(
                "Command error",
                Some(&format!("Wrong option value '{}'", v)),
                Some("Must be of format 'TagName=TagValue'"),
            ));
        }
        map.insert(tag_kv_pair[0].to_string(), tag_kv_pair[1].to_string());
    }
    Ok(map)
}

pub fn get_anonymization_config(
    replace: Option<Vec<&str>>,
    keep: Option<Vec<&str>>,
    keep_private_tags: Option<bool>,
    config_file: Option<&str>,
) -> Result<Option<Anonymization>> {
    // This should never happen, but double-checking anyway
    if (replace.is_some() || keep.is_some() || keep_private_tags.is_some())
        && config_file.is_some()
    {
        return Err(CliError::new(
            "Command error",
            Some("Conflicting options"),
            None,
        ));
    }

    match config_file {
        Some(c) => Ok(Some(get_anonymization_config_from_file(c)?)),
        None => match (&replace, &keep, &keep_private_tags) {
            (None, None, None) => Ok(None),
            // TODO: This assumes that there is always either a config file
            // or at least one of the options
            _ => Ok(Some(get_anonymization_config_from_cmd_options(
                replace,
                keep,
                keep_private_tags,
            )?)),
        },
    }
}

pub fn get_modification_config(
    replace: Option<Vec<&str>>,
    remove: Option<Vec<&str>>,
    config_file: Option<&str>,
) -> Result<Modification> {
    // This should never happen, but double-checking anyway
    if replace.is_none() && remove.is_none() && config_file.is_none() {
        return Err(CliError::new(
            "Command error",
            Some("Not enough options"),
            None,
        ));
    }
    if (replace.is_some() || remove.is_some()) && config_file.is_some() {
        return Err(CliError::new(
            "Command error",
            Some("Conflicting options"),
            None,
        ));
    }

    match config_file {
        Some(c) => Ok(get_modification_config_from_file(c)?),
        None => match (&replace, &remove) {
            // TODO: This assumes that there is always either a config file
            // or at least one of the options
            _ => Ok(get_modification_config_from_cmd_options(replace, remove)?),
        },
    }
}

fn get_anonymization_config_from_file(config_file: &str) -> Result<Anonymization> {
    let yaml = fs::read(config_file)?;
    let mut a: Anonymization = serde_yaml::from_slice(&yaml)?;
    a.force = Some(true);
    Ok(a)
}

fn get_modification_config_from_file(config_file: &str) -> Result<Modification> {
    let yaml = fs::read(config_file)?;
    let mut a: Modification = serde_yaml::from_slice(&yaml)?;
    a.force = Some(true);
    Ok(a)
}

fn get_anonymization_config_from_cmd_options(
    replace: Option<Vec<&str>>,
    keep: Option<Vec<&str>>,
    keep_private_tags: Option<bool>,
) -> Result<Anonymization> {
    Ok(Anonymization {
        replace: match replace {
            Some(r) => Some(parse_tag_kv_pairs(r)?),
            None => None,
        },
        keep: keep.map(|vec| vec.iter().map(ToString::to_string).collect()),
        keep_private_tags,
        dicom_version: None,
        force: Some(true),
    })
}

fn get_modification_config_from_cmd_options(
    replace: Option<Vec<&str>>,
    remove: Option<Vec<&str>>,
) -> Result<Modification> {
    Ok(Modification {
        replace: match replace {
            Some(r) => Some(parse_tag_kv_pairs(r)?),
            None => None,
        },
        remove: remove.map(|vec| vec.iter().map(ToString::to_string).collect()),
        force: Some(true),
    })
}

pub fn get_server_address(cmd_option: Option<&str>) -> result::Result<String, CliError> {
    match cmd_option {
        Some(s) => Ok(s.to_string()),
        None => match env::var("ORC_ORTHANC_SERVER") {
            Ok(s) => Ok(s),
            Err(e) => Err(CliError::new(
                "Command error",
                Some("Neither --server nor ORC_ORTHANC_SERVER are set"),
                Some(&format!("{}", e)),
            )),
        },
    }
}

pub fn get_username(cmd_option: Option<&str>) -> Option<String> {
    match cmd_option {
        Some(s) => Some(s.to_string()),
        None => match env::var("ORC_ORTHANC_USERNAME") {
            Ok(s) => Some(s),
            Err(_) => None, // TODO: This will hide the error
        },
    }
}

pub fn get_password(cmd_option: Option<&str>) -> Option<String> {
    match cmd_option {
        Some(s) => Some(s.to_string()),
        None => match env::var("ORC_ORTHANC_PASSWORD") {
            Ok(s) => Some(s),
            Err(_) => None, // TODO: This will hide the error
        },
    }
}

pub fn get_iap_client_id(cmd_option: Option<&str>) -> Option<String> {
    match cmd_option {
        Some(s) => Some(s.to_string()),
        None => match env::var("IAP_CLIENT_ID") {
            Ok(s) => Some(s),
            Err(_) => None, // TODO: This will hide the error
        },
    }
}

pub fn get_google_application_credentials(cmd_option: Option<&str>) -> Option<String> {
    match cmd_option {
        Some(s) => Some(s.to_string()),
        None => match env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            Ok(s) => Some(s),
            Err(_) => None, // TODO: This will hide the error
        },
    }
}

pub fn check_columns_option(
    original_header: &[&str],
    requested_columns: &[&str],
) -> Result<()> {
    for c in requested_columns {
        if !original_header.contains(c) {
            return Err(CliError::new(
                "Command error",
                Some(&format!(
                    "Invalid column name: {}. Available columns: {}",
                    c,
                    original_header.join(", ")
                )),
                None,
            ));
        }
    }
    Ok(())
}

pub fn get_header_and_dicom_tags(
    header: &mut Vec<&str>,
    dicom_tags: &mut Vec<&str>,
    columns: Option<Vec<&str>>,
) -> Result<()> {
    if let Some(c) = columns {
        check_columns_option(header, &c)?;
        header.retain(|v| c.contains(v));
        dicom_tags.retain(|v| c.contains(v));
    };
    Ok(())
}

pub fn print_table(table: Table) {
    println!("{}", table);
}

pub fn exit_with_error(error: CliError) {
    let output = create_error_table(error);
    eprintln!("{}", output);
    process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use maplit::hashmap;
    use regex::RegexBuilder;
    use std::env::{remove_var, set_var};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn format_table(table: Table) -> String {
        let trailing_whitespace_regex = RegexBuilder::new(r"([ ]+$)")
            .multi_line(true)
            .build()
            .unwrap();
        trailing_whitespace_regex
            .replace_all(&format!("{}", table), "")
            .to_string()
    }

    #[test]
    fn test_create_table_with_header() {
        let table = create_table(Some(&["Foo", "bar"]));
        assert_eq!(&format!("{}", table), " Foo   bar \n-----------");
    }

    #[test]
    fn test_create_table_no_header() {
        let table = create_table(None);
        assert_eq!(&format!("{}", table), "");
    }

    fn test_list_table_patient(
        columns: &[&str],
        dicom_tags: &[&str],
        no_header: bool,
        expected_output: &str,
    ) {
        let patient_1 = Patient {
            id: "foo".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 1, 1).and_hms(15, 46, 17),
            main_dicom_tags: hashmap! {
                "PatientID".to_string() => "foo_id".to_string(),
                "PatientName".to_string() => "Rick Sanchez".to_string(),
            },
            studies: ["study_1".to_string()].to_vec(),
            entity: EntityKind::Patient,
            anonymized_from: None,
        };
        let patient_2 = Patient {
            id: "bar".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 1, 1).and_hms(15, 46, 17),
            main_dicom_tags: hashmap! {
                "PatientName".to_string() => "Morty Smith".to_string(),
            },
            studies: ["study_1".to_string(), "study_2".to_string()].to_vec(),
            entity: EntityKind::Patient,
            anonymized_from: None,
        };
        assert_eq!(
            format_table(create_list_table(
                vec![patient_1, patient_2],
                &columns,
                &dicom_tags,
                no_header,
            )),
            expected_output
        );
    }

    fn test_list_table_study(
        columns: &[&str],
        dicom_tags: &[&str],
        no_header: bool,
        expected_output: &str,
    ) {
        let study_1 = Study {
            id: "foo".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 8, 30).and_hms(19, 11, 09),
            main_dicom_tags: hashmap! {
                "AccessionNumber".to_string() => "foo_an".to_string(),
                "StudyInstanceUID".to_string() => "foo_suid".to_string(),
                "StudyDescription".to_string() => "foo_sd".to_string(),
            },
            parent_patient: "patient_foo".to_string(),
            patient_main_dicom_tags: hashmap! {
                "PatientName".to_string() => "Rick Sanchez".to_string(),
            },
            series: ["foo_series_1".to_string(), "foo_series_2".to_string()].to_vec(),
            entity: EntityKind::Study,
            anonymized_from: None,
        };
        let study_2 = Study {
            id: "bar".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 12, 30).and_hms(10, 10, 0),
            main_dicom_tags: hashmap! {
                "PatientID".to_string() => "bar_pid".to_string(),
                "StudyDate".to_string() => "20200101".to_string(),
                "StudyTime".to_string() => "190001".to_string(),
            },
            parent_patient: "patient_bar".to_string(),
            patient_main_dicom_tags: hashmap! {
                "PatientName".to_string() => "Rick Sanchez".to_string(),
            },
            series: ["bar_series_1".to_string()].to_vec(),
            entity: EntityKind::Study,
            anonymized_from: None,
        };
        assert_eq!(
            format_table(create_list_table(
                vec![study_1, study_2],
                &columns,
                &dicom_tags,
                no_header,
            )),
            expected_output
        );
    }

    fn test_list_table_series(
        columns: &[&str],
        dicom_tags: &[&str],
        no_header: bool,
        expected_output: &str,
    ) {
        let series_1 = Series {
            id: "foo".to_string(),
            status: "Unknown".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 8, 30).and_hms(19, 11, 09),
            main_dicom_tags: hashmap! {
                "BodyPartExamined".to_string() => "ABDOMEN".to_string(),
                "SeriesDescription".to_string() => "Foo".to_string(),
            },
            parent_study: "study_1".to_string(),
            expected_number_of_instances: Some(17),
            instances: ["i1".to_string(), "i2".to_string()].to_vec(),
            entity: EntityKind::Series,
            anonymized_from: None,
        };
        let series_2 = Series {
            id: "bar".to_string(),
            status: "Known".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2021, 8, 30).and_hms(19, 12, 09),
            main_dicom_tags: hashmap! {
                "BodyPartExamined".to_string() => "KNEE".to_string(),
                "Modality".to_string() => "CT".to_string(),
            },
            parent_study: "study_2".to_string(),
            expected_number_of_instances: Some(17),
            instances: ["i3".to_string(), "i4".to_string()].to_vec(),
            entity: EntityKind::Series,
            anonymized_from: None,
        };
        assert_eq!(
            format_table(create_list_table(
                vec![series_1, series_2],
                &columns,
                &dicom_tags,
                no_header,
            )),
            expected_output
        );
    }
    fn test_list_table_instance(
        columns: &[&str],
        dicom_tags: &[&str],
        no_header: bool,
        expected_output: &str,
    ) {
        let instance_1 = Instance {
            id: "foo".to_string(),
            main_dicom_tags: hashmap! {
                "SOPInstanceUID".to_string() => "suid_1".to_string(),
                "InstanceNumber".to_string() => "in_1".to_string(),
            },
            parent_series: "foo_series".to_string(),
            index_in_series: Some(13),
            file_uuid: "file_uuid".to_string(),
            file_size: 139402,
            modified_from: None,
            entity: EntityKind::Instance,
            anonymized_from: None,
        };
        let instance_2 = Instance {
            id: "bar".to_string(),
            main_dicom_tags: hashmap! {
                "SOPInstanceUID".to_string() => "suid_2".to_string(),
                "InstanceCreationDate".to_string() => "19000101".to_string(),
                "InstanceCreationTime".to_string() => "000000".to_string(),
            },
            parent_series: "foo_series".to_string(),
            index_in_series: Some(13),
            file_uuid: "file_uuid".to_string(),
            file_size: 139402,
            modified_from: None,
            entity: EntityKind::Instance,
            anonymized_from: None,
        };
        assert_eq!(
            format_table(create_list_table(
                vec![instance_1, instance_2],
                &columns,
                &dicom_tags,
                no_header,
            )),
            expected_output
        );
    }

    #[test]
    fn test_create_list_table_patient() {
        test_list_table_patient(
            PATIENTS_LIST_HEADER,
            PATIENTS_LIST_DICOM_TAGS,
            false,
            include_str!("../tests/data/unit/list_patients").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_patient_no_header() {
        test_list_table_patient(
            PATIENTS_LIST_HEADER,
            PATIENTS_LIST_DICOM_TAGS,
            true,
            include_str!("../tests/data/unit/list_patients_no_header").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_patient_columns() {
        test_list_table_patient(
            &["PatientID", "Number of Studies"],
            &["PatientID"],
            false,
            include_str!("../tests/data/unit/list_patients_columns").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_patient_columns_no_header() {
        test_list_table_patient(
            &["PatientID", "Number of Studies"],
            &["PatientID"],
            true,
            include_str!("../tests/data/unit/list_patients_columns_no_header").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_study() {
        test_list_table_study(
            STUDIES_LIST_HEADER,
            STUDIES_LIST_DICOM_TAGS,
            false,
            include_str!("../tests/data/unit/list_studies").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_study_no_header() {
        test_list_table_study(
            STUDIES_LIST_HEADER,
            STUDIES_LIST_DICOM_TAGS,
            true,
            include_str!("../tests/data/unit/list_studies_no_header").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_series() {
        test_list_table_series(
            SERIES_LIST_HEADER,
            SERIES_LIST_DICOM_TAGS,
            false,
            include_str!("../tests/data/unit/list_series").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_series_no_header() {
        test_list_table_series(
            SERIES_LIST_HEADER,
            SERIES_LIST_DICOM_TAGS,
            true,
            include_str!("../tests/data/unit/list_series_no_header").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_instance() {
        test_list_table_instance(
            INSTANCES_LIST_HEADER,
            INSTANCES_LIST_DICOM_TAGS,
            false,
            include_str!("../tests/data/unit/list_instances").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_instance_no_header() {
        test_list_table_instance(
            INSTANCES_LIST_HEADER,
            INSTANCES_LIST_DICOM_TAGS,
            true,
            include_str!("../tests/data/unit/list_instances_no_header").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_instance_columns() {
        test_list_table_instance(
            &["SOPInstanceUID", "Index in series", "File size"],
            &["SOPInstanceUID"],
            false,
            include_str!("../tests/data/unit/list_instances_columns").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_instance_columns_no_header() {
        test_list_table_instance(
            &["SOPInstanceUID", "Index in series", "File size"],
            &["SOPInstanceUID"],
            true,
            include_str!("../tests/data/unit/list_instances_columns_no_header").trim_end(),
        );
    }

    #[test]
    fn test_create_list_table_no_header_no_data() {
        let data: Vec<Patient> = vec![];
        assert_eq!(
            format_table(create_list_table(
                data,
                PATIENTS_LIST_HEADER,
                PATIENTS_LIST_DICOM_TAGS,
                true,
            )),
            ""
        );
    }

    #[test]
    fn test_create_show_table_patient() {
        let patient = Patient {
            id: "foo".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 1, 1).and_hms(15, 46, 17),
            main_dicom_tags: hashmap! {
                "PatientID".to_string() => "foo_id".to_string(),
                "PatientName".to_string() => "Rick Sanchez".to_string(),
            },
            studies: ["study_1".to_string()].to_vec(),
            entity: EntityKind::Patient,
            anonymized_from: None,
        };

        assert_eq!(
            format_table(create_show_table(patient, &PATIENT_DICOM_TAGS)),
            include_str!("../tests/data/unit/show_patient").trim_end()
        );
    }

    #[test]
    fn test_create_show_table_study() {
        let study = Study {
            id: "foo".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 8, 30).and_hms(19, 11, 09),
            main_dicom_tags: hashmap! {
                "AccessionNumber".to_string() => "foo_an".to_string(),
                "StudyInstanceUID".to_string() => "foo_suid".to_string(),
                "StudyDescription".to_string() => "foo_sd".to_string(),
            },
            parent_patient: "patient_foo".to_string(),
            patient_main_dicom_tags: hashmap! {
                "PatientName".to_string() => "Rick Sanchez".to_string(),
            },
            series: ["foo_series_1".to_string(), "foo_series_2".to_string()].to_vec(),
            entity: EntityKind::Study,
            anonymized_from: None,
        };

        assert_eq!(
            format_table(create_show_table(study, &STUDY_DICOM_TAGS)),
            include_str!("../tests/data/unit/show_study").trim_end()
        );
    }

    #[test]
    fn test_create_show_table_series() {
        let series = Series {
            id: "foo".to_string(),
            status: "Unknown".to_string(),
            is_stable: true,
            last_update: NaiveDate::from_ymd(2020, 8, 30).and_hms(19, 11, 09),
            main_dicom_tags: hashmap! {
                "BodyPartExamined".to_string() => "ABDOMEN".to_string(),
                "SeriesDescription".to_string() => "Foo".to_string(),
            },
            parent_study: "study_1".to_string(),
            expected_number_of_instances: Some(17),
            instances: ["i1".to_string(), "i2".to_string()].to_vec(),
            entity: EntityKind::Series,
            anonymized_from: None,
        };
        assert_eq!(
            format_table(create_show_table(series, &SERIES_DICOM_TAGS)),
            include_str!("../tests/data/unit/show_series").trim_end()
        );
    }

    #[test]
    fn test_create_show_table_instance() {
        let instance = Instance {
            id: "foo".to_string(),
            main_dicom_tags: hashmap! {
                "SOPInstanceUID".to_string() => "suid_1".to_string(),
                "InstanceNumber".to_string() => "in_1".to_string(),
            },
            parent_series: "foo_series".to_string(),
            index_in_series: Some(13),
            file_uuid: "file_uuid".to_string(),
            file_size: 139402,
            modified_from: None,
            entity: EntityKind::Instance,
            anonymized_from: None,
        };
        assert_eq!(
            format_table(create_show_table(instance, &INSTANCE_DICOM_TAGS)),
            include_str!("../tests/data/unit/show_instance").trim_end()
        );
    }

    #[test]
    fn test_create_new_entity_table() {
        let res = ModificationResult {
            id: "foobar".to_string(),
            patient_id: "bazqux".to_string(),
            path: "long_and_rocky".to_string(),
            entity: EntityKind::Study,
        };
        let expected_table = " New Study ID   foobar \n Patient ID     bazqux ";
        assert_eq!(format!("{}", create_new_entity_table(res)), expected_table)
    }

    #[test]
    fn test_create_new_entity_table_patient() {
        let res = ModificationResult {
            id: "foobar".to_string(),
            patient_id: "bazqux".to_string(),
            path: "long_and_rocky".to_string(),
            entity: EntityKind::Patient,
        };
        let expected_table = " New Patient ID   foobar ";
        assert_eq!(format!("{}", create_new_entity_table(res)), expected_table)
    }

    #[test]
    fn test_create_error_table() {
        assert_eq!(
            format!(
                "{}",
                create_error_table(CliError::new(
                    "error",
                    Some("message"),
                    Some("details")
                ))
            ),
            " Error     error   \n Message   message \n Details   details "
        );
        assert_eq!(
            format!(
                "{}",
                create_error_table(CliError::new("error", None, Some("details")))
            ),
            " Error     error   \n Details   details "
        );
        assert_eq!(
            format!(
                "{}",
                create_error_table(CliError::new("error", Some("message"), None))
            ),
            " Error     error   \n Message   message "
        );
        assert_eq!(
            format!("{}", create_error_table(CliError::new("error", None, None))),
            " Error   error "
        );
    }

    #[test]
    fn test_parse_tag_kv_pairs() {
        assert_eq!(
            parse_tag_kv_pairs(vec!["Foo=Bar", "Baz=42"]).unwrap(),
            hashmap! {"Foo".to_string() => "Bar".to_string(), "Baz".to_string() => "42".to_string()}
        )
    }

    #[test]
    fn test_parse_tag_kv_pairs_error() {
        assert_eq!(
            parse_tag_kv_pairs(vec!["Foo=Bar", "Baz"]).unwrap_err(),
            CliError::new(
                "Command error",
                Some("Wrong option value 'Baz'"),
                Some("Must be of format 'TagName=TagValue'"),
            )
        )
    }

    #[test]
    fn test_get_anonymization_config_from_file() {
        let mut file = fs::File::create("/tmp/anon_config.yml").unwrap();
        file.write(b"{}").unwrap();
        assert_eq!(
            get_anonymization_config_from_file("/tmp/anon_config.yml").unwrap(),
            Anonymization {
                replace: None,
                keep: None,
                keep_private_tags: None,
                force: Some(true),
                dicom_version: None
            }
        )
    }

    #[test]
    fn test_get_anonymization_config_from_file_file_not_found() {
        assert_eq!(
            get_anonymization_config_from_file("/tmp/anon_garble.yml").unwrap_err(),
            CliError {
                error: "No such file or directory (os error 2)".to_string(),
                message: None,
                details: None
            }
        )
    }

    #[test]
    fn test_get_anonymization_config_from_file_yaml_parse_error() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "garble").unwrap();
        assert_eq!(
            get_anonymization_config_from_file(file.path().to_str().unwrap()).unwrap_err(),
            CliError {
                error: "invalid type: string \"garble\", expected struct Anonymization at line 1 column 1".to_string(),
                message: None,
                details: None
            }
        )
    }

    #[test]
    fn test_get_anonymization_config_from_cmd_options() {
        assert_eq!(
            get_anonymization_config_from_cmd_options(
                Some(vec!["Foo=Bar", "Baz=qux"]),
                Some(vec!["Qux", "Quuz"]),
                Some(true)
            )
            .unwrap(),
            Anonymization {
                replace: Some(
                    hashmap! {"Foo".to_string() => "Bar".to_string(), "Baz".to_string() => "qux".to_string()}
                ),
                keep: Some(vec!["Qux".to_string(), "Quuz".to_string()]),
                keep_private_tags: Some(true),
                dicom_version: None,
                force: Some(true)
            }
        );

        assert_eq!(
            get_anonymization_config_from_cmd_options(None, None, None).unwrap(),
            Anonymization {
                replace: None,
                keep: None,
                keep_private_tags: None,
                dicom_version: None,
                force: Some(true)
            }
        )
    }

    #[test]
    fn test_get_modification_config_from_cmd_options() {
        assert_eq!(
            get_modification_config_from_cmd_options(
                Some(vec!["Foo=Bar", "Baz=qux"]),
                Some(vec!["Qux", "Quuz"]),
            )
            .unwrap(),
            Modification {
                replace: Some(
                    hashmap! {"Foo".to_string() => "Bar".to_string(), "Baz".to_string() => "qux".to_string()}
                ),
                remove: Some(vec!["Qux".to_string(), "Quuz".to_string()]),
                force: Some(true)
            }
        );

        assert_eq!(
            get_modification_config_from_cmd_options(None, None).unwrap(),
            Modification {
                replace: None,
                remove: None,
                force: Some(true)
            }
        );
    }

    #[test]
    fn test_get_modification_config_from_file() {
        let mut file = fs::File::create("/tmp/mod_config.yml").unwrap();
        file.write(b"{}").unwrap();
        assert_eq!(
            get_modification_config_from_file("/tmp/mod_config.yml").unwrap(),
            Modification {
                replace: None,
                remove: None,
                force: Some(true)
            }
        )
    }

    #[test]
    fn test_get_modification_config_from_file_file_not_found() {
        assert_eq!(
            get_modification_config_from_file("/tmp/anon_garble.yml").unwrap_err(),
            CliError {
                error: "No such file or directory (os error 2)".to_string(),
                message: None,
                details: None
            }
        )
    }

    #[test]
    fn test_get_modification_config_from_file_yaml_parse_error() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "garble").unwrap();
        assert_eq!(
            get_modification_config_from_file(file.path().to_str().unwrap()).unwrap_err(),
            CliError {
                error: "invalid type: string \"garble\", expected struct Modification at line 1 column 1".to_string(),
                message: None,
                details: None
            }
        )
    }

    #[test]
    fn test_get_anonymization_config_conflicting_options() {
        assert_eq!(
            get_anonymization_config(
                Some(vec!["Foo=Bar", "Baz=qux"]),
                Some(vec!["Qux", "Quuz"]),
                Some(true),
                Some("/tmp/foo.yml")
            )
            .unwrap_err(),
            CliError::new("Command error", Some("Conflicting options"), None)
        )
    }

    #[test]
    fn test_get_modification_config_not_enough_options() {
        assert_eq!(
            get_modification_config(None, None, None).unwrap_err(),
            CliError::new("Command error", Some("Not enough options"), None)
        )
    }

    #[test]
    fn test_get_modification_config_conflicting_options() {
        assert_eq!(
            get_modification_config(
                Some(vec!["Foo=Bar", "Baz=qux"]),
                Some(vec!["Qux", "Quuz"]),
                Some("/tmp/foo.yml")
            )
            .unwrap_err(),
            CliError::new("Command error", Some("Conflicting options"), None)
        )
    }

    #[test]
    fn test_get_server() {
        remove_var("ORC_ORTHANC_SERVER");
        assert_eq!(get_server_address(Some("foo")).unwrap(), "foo".to_string());
        assert_eq!(
            get_server_address(None).unwrap_err(),
            CliError::new(
                "Command error",
                Some("Neither --server nor ORC_ORTHANC_SERVER are set"),
                Some("environment variable not found"),
            )
        );
        set_var("ORC_ORTHANC_SERVER", "bar");
        assert_eq!(get_server_address(None).unwrap(), "bar".to_string());
        assert_eq!(get_server_address(Some("baz")).unwrap(), "baz".to_string())
    }

    #[test]
    fn test_get_username() {
        remove_var("ORC_ORTHANC_USERNAME");
        assert_eq!(get_username(Some("foo")).unwrap(), "foo".to_string());
        assert_eq!(get_username(None), None);
        set_var("ORC_ORTHANC_USERNAME", "bar");
        assert_eq!(get_username(Some("foo")).unwrap(), "foo".to_string());
        assert_eq!(get_username(None).unwrap(), "bar".to_string());
    }

    #[test]
    fn test_get_password() {
        remove_var("ORC_ORTHANC_PASSWORD");
        assert_eq!(get_password(Some("foo")).unwrap(), "foo".to_string());
        assert_eq!(get_password(None), None);
        set_var("ORC_ORTHANC_PASSWORD", "bar");
        assert_eq!(get_password(Some("foo")).unwrap(), "foo".to_string());
        assert_eq!(get_password(None).unwrap(), "bar".to_string());
    }

    #[test]
    fn test_check_columns_options() {
        assert_eq!(
            check_columns_option(&["foo", "bar", "baz"], &["foo", "baz"]).unwrap(),
            ()
        );

        assert_eq!(
            check_columns_option(&["foo", "bar", "baz"], &["bar", "qux"]).unwrap_err(),
            CliError::new(
                "Command error",
                Some("Invalid column name: qux. Available columns: foo, bar, baz"),
                None
            )
        )
    }

    #[test]
    fn test_get_header_and_dicom_tags() {
        let mut header = vec!["foo", "bar", "baz", "qux", "quux", "quuz"];
        let mut dicom_tags = vec!["qux", "quux", "quuz"];

        get_header_and_dicom_tags(
            &mut header,
            &mut dicom_tags,
            Some(vec!["bar", "quux", "quuz"]),
        )
        .unwrap();

        assert_eq!(header, vec!["bar", "quux", "quuz"]);
        assert_eq!(dicom_tags, vec!["quux", "quuz"]);
    }

    #[test]
    fn test_get_header_and_dicom_tags_no_columns() {
        let mut header = vec!["foo", "bar", "baz"];
        let mut dicom_tags = vec!["qux", "quux", "quuz"];

        get_header_and_dicom_tags(&mut header, &mut dicom_tags, None).unwrap();

        assert_eq!(header, vec!["foo", "bar", "baz"]);
        assert_eq!(dicom_tags, vec!["qux", "quux", "quuz"]);
    }
}
