pub fn print_startup_banner() {
    let year = chrono::Local::now().format("%Y").to_string();

    // ANSI color codes
    let red = "\x1B[38;5;196m";
    let gray = "\x1B[38;5;245m";
    let bright_red = "\x1B[91m";
    let reset = "\x1B[0m";

    println!(
        r#"
  {year} PsychonautWiki
   {red}
          /\,%_\
          \%/,\          {gray}Old age should burn{red}
       _.-"%%|//%    {gray}and rave at close of day{red}
      .'  .-"  /%%%
  _.-'_.-" 0)   \%%%     {gray}Rage, rage against{red}
 /.\.'          \%%%  {gray}the dying of the light{red}
 \ /      _,      %%%
  `"--"~`\   _,*'\%'   _,--""""-,%%,
         )*^     `""~~`           \%%%,
         _/                          \%%%
     _.-`/                           |%%,___
 _.-"   /      ,              ,     ,|%%   .`\
/\     /      /                `\     \%'   \ /
\ \ _,/      /`~.-._          _,`\     \`""~~`
 `"` /-.`_, /'      `~----"~     `\     \
     \___,'                        \.-"`/
                                    `--'
         {bright_red}bifrost v3.0{reset}
"#,
        year = year,
        red = red,
        gray = gray,
        bright_red = bright_red,
        reset = reset
    );
}
