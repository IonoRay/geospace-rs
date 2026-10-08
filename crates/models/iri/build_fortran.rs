use std::{
    env,
    error::Error,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::build_manifest::SOURCES;

pub(crate) fn compile(root: &Path) -> Result<(), Box<dyn Error>> {
    let build = root.join("build");
    fs::create_dir_all(&build)?;
    let compiler = env::var_os("FC").unwrap_or_else(|| OsStr::new("gfortran").to_owned());
    let mut command = Command::new(&compiler);
    command
        .current_dir(&build)
        .args([
            "-c",
            "-O2",
            "-fPIC",
            "-std=legacy",
            "-ffixed-line-length-none",
            "-fallow-argument-mismatch",
            "-J",
        ])
        .arg(&build)
        .arg("-I")
        .arg(&build)
        .arg(root.join("ionoray_iri2020_assets.f90"));
    for (source, _) in SOURCES {
        command.arg(root.join(source));
    }
    command.arg(root.join("ionoray_iri2020.f90"));
    if env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        command.arg("-fno-omit-frame-pointer");
    }
    run(&mut command, "Fortran compilation")?;

    let archiver = env::var_os("AR").unwrap_or_else(|| OsStr::new("ar").to_owned());
    let mut archive = Command::new(archiver);
    archive
        .current_dir(&build)
        .args(["crus", "libionoray_iri2020.a", "ionoray_iri2020_assets.o"]);
    for (source, _) in SOURCES {
        archive.arg(format!("{}.o", source.trim_end_matches(".for")));
    }
    archive.arg("ionoray_iri2020.o");
    run(&mut archive, "Fortran archive creation")?;
    println!("cargo:rustc-link-search=native={}", build.display());
    println!("cargo:rustc-link-lib=static=ionoray_iri2020");
    add_runtime_search(&compiler, "libgfortran")?;
    println!("cargo:rustc-link-lib=dylib=gfortran");
    Ok(())
}

fn add_runtime_search(compiler: &OsStr, library: &str) -> Result<(), Box<dyn Error>> {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let filename = match target_os.as_str() {
        "macos" => format!("{library}.dylib"),
        "windows" => format!("{library}.dll.a"),
        _ => format!("{library}.so"),
    };
    let output = Command::new(compiler)
        .arg(format!("-print-file-name={filename}"))
        .output()?;
    if !output.status.success() {
        return Err("gfortran could not locate its runtime library".into());
    }
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    if let Some(parent) = path.parent().filter(|_| path.is_absolute()) {
        println!("cargo:rustc-link-search=native={}", parent.display());
    }
    Ok(())
}

fn run(command: &mut Command, operation: &str) -> Result<(), Box<dyn Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{operation} failed with {status}").into());
    }
    Ok(())
}

pub(crate) const ASSET_MODULE: &str = r#"
module ionoray_iri2020_assets
  use iso_c_binding
  implicit none
  character(len=:), allocatable :: asset_root
contains
  subroutine ionoray_iri2020_set_asset_root(path, path_len, status) &
      bind(C, name="ionoray_iri2020_set_asset_root")
    character(kind=c_char), intent(in) :: path(*)
    integer(c_int), value :: path_len
    integer(c_int), intent(out) :: status
    integer :: i
    if (path_len <= 0) then
      status = 1_c_int
      return
    end if
    if (allocated(asset_root)) deallocate(asset_root)
    allocate(character(len=path_len) :: asset_root)
    do i = 1, path_len
      asset_root(i:i) = path(i)
    end do
    status = 0_c_int
  end subroutine

  function iri_asset_path(name) result(path)
    character(len=*), intent(in) :: name
    character(len=:), allocatable :: path
    if (.not. allocated(asset_root)) error stop "IRI-2020 asset root is not initialized"
    path = asset_root // '/' // trim(adjustl(name))
  end function
end module
"#;

pub(crate) const C_SHIM: &str = r#"
module ionoray_iri2020_c_api
  use iso_c_binding
  implicit none
contains
  subroutine ionoray_iri2020_init(path, path_len, status) &
      bind(C, name="ionoray_iri2020_init")
    character(kind=c_char), intent(in) :: path(*)
    integer(c_int), value :: path_len
    integer(c_int), intent(out) :: status
    interface
      subroutine set_root(path, path_len, status) &
          bind(C, name="ionoray_iri2020_set_asset_root")
        use iso_c_binding
        character(kind=c_char), intent(in) :: path(*)
        integer(c_int), value :: path_len
        integer(c_int), intent(out) :: status
      end subroutine
    end interface
    call set_root(path, path_len, status)
  end subroutine

  subroutine ionoray_iri2020_eval(year, mmdd, ut_hours, latitude, longitude, &
      altitude_km, rz12, ig12, f107_daily, f107_81_day, output, status) &
      bind(C, name="ionoray_iri2020_eval")
    integer(c_int), value :: year, mmdd
    real(c_float), value :: ut_hours, latitude, longitude, altitude_km
    real(c_float), value :: rz12, ig12, f107_daily, f107_81_day
    real(c_float), intent(out) :: output(20)
    integer(c_int), intent(out) :: status
    logical :: jf(50)
    real :: outf(20, 1000), oarr(100)

    jf = .true.
    jf([4, 5, 6, 17, 21, 22, 23, 25, 26, 27, 28, 30, 32, 33, 34, 35, &
        39, 40, 45, 47]) = .false.
    outf = -1.0
    oarr = -1.0
    oarr(33) = rz12
    oarr(39) = ig12
    oarr(41) = f107_daily
    oarr(46) = f107_81_day
    call iri_sub(jf, 0, latitude, longitude, year, mmdd, ut_hours + 25.0, &
        altitude_km, altitude_km, 1.0, outf, oarr)

    output(1:11) = outf(1:11, 1)
    output(12:17) = oarr(1:6)
    output(18) = oarr(23)
    output(19) = oarr(25)
    output(20) = oarr(27)
    status = 0_c_int
  end subroutine
end module
"#;
