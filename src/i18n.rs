//! UI language support: English (default) and Türkçe.
//!
//! Turkish strings deliberately follow FM's own Turkish game localization
//! (attribute names, morale words, finance terms), not generic translation.
//! The 2026-11-07 live session against a Turkish FM24 Epic career confirmed
//! the terms marked with a witness note in the table; the rest follow FM's
//! stable Turkish localization and are easy to correct in one place here.
//!
//! `tr` maps whole English literals through a dictionary so partial coverage
//! is safe: an unmapped string renders unchanged in English, never garbage.

use std::collections::HashMap;
use std::sync::LazyLock;

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Language {
    #[default]
    English,
    Turkish,
}

impl Language {
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Turkish => "Türkçe",
        }
    }
}

pub fn set_language(ctx: &egui::Context, lang: Language) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("fm_language"), lang));
}

pub fn language(ctx: &egui::Context) -> Language {
    ctx.data_mut(|d| d.get_temp(egui::Id::new("fm_language")))
        .unwrap_or_default()
}

/// Translate an English UI/game literal into the active language.
/// Unknown strings pass through unchanged; English is identity.
pub fn tr(ctx: &egui::Context, s: &str) -> String {
    if language(ctx) == Language::English {
        return s.to_owned();
    }
    TR_TABLE
        .get(s)
        .map(|t| t.to_string())
        .unwrap_or_else(|| s.to_owned())
}

/// `ui.tr("Players")` shorthand for render sites.
pub trait UiTr {
    fn tr(&self, s: &str) -> String;
}

impl UiTr for egui::Ui {
    fn tr(&self, s: &str) -> String {
        tr(self.ctx(), s)
    }
}

/// Wage suffix per language for `settings::format_wage`.
pub fn wage_suffix(period: crate::settings::WagePeriod, lang: Language) -> &'static str {
    match (lang, period) {
        (Language::Turkish, crate::settings::WagePeriod::Weekly) => "/hafta",
        (Language::Turkish, crate::settings::WagePeriod::Monthly) => "/ay",
        (Language::Turkish, crate::settings::WagePeriod::Yearly) => "/yıl",
        (_, crate::settings::WagePeriod::Weekly) => "/wk",
        (_, crate::settings::WagePeriod::Monthly) => "/mo",
        (_, crate::settings::WagePeriod::Yearly) => "/yr",
    }
}

static TR_TABLE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let pairs: &[(&'static str, &'static str)] = &[
        // ── Navigation / app chrome ───────────────────────────────────────
        ("BROWSE", "GÖZAT"),
        ("MANAGEMENT", "YÖNETİM"),
        ("GAME CONNECTION", "OYUN BAĞLANTISI"),
        ("Best XI", "En İyi 11"),
        ("Players", "Oyuncular"),
        ("Staff", "Personel"),
        ("Clubs", "Kulüpler"),
        ("Human Manager", "Menajer"),
        ("Load Game", "Oyunu Yükle"),
        ("Auto-connect", "Otomatik bağlan"),
        ("Connected", "Bağlı"),
        ("Disconnected", "Bağlantı kesik"),
        ("Waiting for fm.exe…", "fm.exe bekleniyor…"),
        ("Waiting for save…", "Kayıt bekleniyor…"),
        ("Game not running", "Oyun çalışmıyor"),
        ("Reconnect", "Yeniden Bağlan"),
        ("Attach:", "Bağlan:"),
        ("Reload Save", "Kaydı Yeniden Yükle"),
        ("Load Save", "Kaydı Yükle"),
        ("Quit", "Çıkış"),
        ("Nothing to reload", "Yüklenecek bir şey yok"),
        ("Reload from memory", "Bellekten yeniden yükle"),
        ("File", "Dosya"),
        ("Settings", "Ayarlar"),
        ("Theme", "Tema"),
        ("Scale", "Boyut"),
        ("Language", "Dil"),
        ("Wage Display", "Maaş Gösterimi"),
        ("Face Pack", "Yüz Paketi"),
        ("Dark", "Koyu"),
        ("Light", "Açık"),
        ("Dark Blue", "Lacivert"),
        ("Weekly", "Haftalık"),
        ("Monthly", "Aylık"),
        ("Yearly", "Yıllık"),
        ("Reload on tab switch", "Sekme değişince yeniden oku"),
        ("Undo", "Geri Al"),
        ("Change history", "Değişiklik Geçmişi"),
        ("Compare players", "Oyuncuları Karşılaştır"),
        ("Refresh", "Yenile"),
        ("Back", "Geri"),
        ("Forward", "İleri"),
        ("Reload", "Yeniden Yükle"),
        ("Apply", "Uygula"),
        ("Cancel", "İptal"),
        ("Clear All", "Tümünü Temizle"),
        ("Reset all", "Tümünü sıfırla"),
        ("No results.", "Sonuç yok."),
        ("Search", "Ara"),
        ("Name/UID:", "İsim/UID:"),
        ("Name or exact UID", "İsim veya tam UID"),
        ("Club:", "Kulüp:"),
        ("Club: Unknown", "Kulüp: Bilinmiyor"),
        ("Open club", "Kulübü aç"),
        ("Unknown", "Bilinmiyor"),
        ("Age", "Yaş"),
        ("Name", "İsim"),
        ("Role", "Görev"),
        ("Position", "Pozisyon"),
        ("Value", "Değer"),
        ("Wage", "Maaş"),
        ("All Roles", "Tüm Görevler"),
        ("CA ≥", "GA ≥"),
        ("PA ≥", "PA ≥"),
        ("Age ≥", "Yaş ≥"),
        ("UID", "UID"),
        // ── Status banners ────────────────────────────────────────────────
        (
            "Editing live — changes apply immediately. Ctrl+Z to undo.",
            "Canlı düzenleme — değişiklikler anında uygulanır. Geri almak için Ctrl+Z.",
        ),
        (
            "Read-only — editing is disabled until a game and save are connected.",
            "Salt okunur — bir oyun ve kayıt bağlanana dek düzenleme kapalı.",
        ),
        (
            "Applying changes to game memory…",
            "Değişiklikler oyun belleğine uygulanıyor…",
        ),
        // ── Categories (Category Display order) ───────────────────────────
        ("Overview", "Genel Bakış"),
        ("Identity", "Kimlik"),
        ("Positions & Traits", "Pozisyonlar ve Özellikler"),
        ("Coaching", "Antrenörlük"),
        ("Technical", "Teknik"),
        ("Mental", "Zihinsel"),
        ("Physical", "Fiziksel"),
        ("Goalkeeping", "Kalecilik"),
        ("Set Pieces", "Duran Toplar"),
        ("Personality", "Kişilik"),
        ("Condition", "Kondisyon"),
        ("Contract", "Sözleşme"),
        ("Finance", "Finans"),
        ("General", "Genel"),
        ("Knowledge", "Bilgi"),
        ("Medical", "Sağlık"),
        ("Reputation", "İtibar"),
        ("Staff Roles", "Görevler"),
        ("Tactical Style", "Taktik Stil"),
        ("Board Relations", "Yönetim İlişkileri"),
        ("Facilities", "Tesisler"),
        ("Positions", "Pozisyonlar"),
        ("Traits", "Özellikler"),
        ("Footedness", "Ayak Kullanımı"),
        ("Ability", "Yetenek"),
        // ── Technical attributes (FM Turkish) ─────────────────────────────
        ("Corners", "Korner"),
        ("Crossing", "Orta Yapma"), // witnessed 2026-11-08 game screen
        ("Dribbling", "Dripling"),
        ("Finishing", "Bitiricilik"),   // witnessed 2026-11-07
        ("First Touch", "İlk Kontrol"), // witnessed 2026-11-08 game screen
        ("Free Kicks", "Serbest Vuruş Kullanma"),
        ("Heading", "Kafa Vuruşu"), // witnessed 2026-11-07
        ("Long Shots", "Uzaktan Şut"),
        ("Long Throws", "Uzun Taç"),
        ("Marking", "Markaj"),
        ("Passing", "Pas"), // witnessed 2026-11-07
        ("Penalty Taking", "Penaltı Kullanma"),
        ("Tackling", "Top Kapma"), // witnessed 2026-11-07
        ("Technique", "Teknik"),   // witnessed 2026-11-07
        // ── Mental attributes ─────────────────────────────────────────────
        ("Aggression", "Agresiflik"),
        ("Anticipation", "Önsezi"),
        ("Bravery", "Cesaret"),
        ("Composure", "Soğukkanlılık"),
        ("Concentration", "Konsantrasyon"),
        ("Decisions", "Karar Alma"),
        ("Determination", "Kararlılık"),
        ("Flair", "Özel Yetenek"),
        ("Leadership", "Liderlik"),
        ("Off The Ball", "Topsuz Alan"),
        ("Positioning", "Pozisyon Alma"),
        ("Teamwork", "Takım Oyunu"),
        ("Vision", "Vizyon"),
        ("Work Rate", "Çalışkanlık"),
        // ── Physical attributes ───────────────────────────────────────────
        ("Acceleration", "Hızlanma"),
        ("Agility", "Çeviklik"),
        ("Balance", "Denge"),
        ("Jumping Reach", "Zıplama"),
        ("Natural Fitness", "Vücut Zindeliği"),
        ("Pace", "Hız"),
        ("Stamina", "Dayanıklılık"),
        ("Strength", "Güç"),
        // ── Goalkeeping attributes ────────────────────────────────────────
        ("Aerial Reach", "Hava Topu Erişimi"),
        ("Command of Area", "Ceza Sahası Hakimiyeti"),
        ("Communication", "İletişim"),
        ("Eccentricity", "Tuhaf Davranışlar"),
        ("Handling", "Top Tutma"),
        ("Kicking", "Vuruş"),
        ("One on Ones", "Bire Bir"),
        ("Punching", "Yumrukla Uzaklaştırma"),
        ("Reflexes", "Refleks"),
        ("Rushing Out", "Çıkış Yapma"),
        ("Throwing", "El Atışı"),
        // ── Personality (FM Turkish) ──────────────────────────────────────
        ("Adaptability", "Uyum Sağlama"),
        ("Ambition", "Hırs"),
        ("Controversy", "Tartışma"),
        ("Loyalty", "Sadakat"),
        ("Pressure", "Baskı"),
        ("Professionalism", "Profesyonellik"),
        ("Sportsmanship", "Sporcu Ruhu"),
        ("Temperament", "Mizaç"),
        // ── Identity / contract fields ────────────────────────────────────
        ("Full Name", "Tam İsim"),
        ("Nickname", "Lakap"),
        ("Date of Birth", "Doğum Tarihi"),
        ("Place of Birth", "Doğum Yeri"),
        ("Nation", "Ülke"),
        ("Nationality", "Uyruk"),
        ("Height", "Boy"),
        ("Weight", "Ağırlık"),
        ("Left Foot", "Sol Ayak"),
        ("Right Foot", "Sağ Ayak"),
        ("International Caps", "Milli Maç"),
        ("International Goals", "Milli Gol"),
        ("U21 Caps", "U21 Milli Maç"),
        ("U21 Goals", "U21 Gol"),
        ("Snr Apps", "Kıdemli Maç"),
        ("Snr Goals", "Kıdemli Gol"),
        ("Current Ability", "Güncel Yetenek"),
        ("Potential Ability", "Potansiyel Yetenek"),
        ("Current Rep", "Güncel İtibar"),
        ("Home Rep", "Yurtiçi İtibar"),
        ("World Rep", "Dünya İtibarı"),
        ("Market Value", "Piyasa Değeri"),
        ("Listed Since", "Liste Başlangıcı"),
        ("Listing Code", "Liste Kodu"),
        ("Listing Date", "Liste Tarihi"),
        ("Listing Flags", "Liste Bayrakları"),
        ("Other %", "Diğer %"),
        ("Passion", "Tutku"),
        ("Role Ratings", "Görev Puanları"),
        ("Social Media", "Sosyal Medya"),
        ("Squad Record", "Kadro Kaydı"),
        ("Squad Rotation", "Kadro Rotasyonu"),
        ("Trigger Press", "Baskı Tetikleme"),
        ("Youth Fac", "Altyapı Tesisleri"),
        ("Morale", "Moral"),
        ("Fatigue", "Yorgunluk"),
        ("Sharpness", "Keskinlik"),
        ("Fitness", "Form"),
        ("Contract Type", "Sözleşme Türü"),
        ("Contract Expiry", "Sözleşme Bitişi"),
        ("Contract Start", "Sözleşme Başlangıcı"),
        ("Contract Signed", "İmza Tarihi"),
        ("Wage (£ p/w)", "Maaş (£/hafta)"),
        ("Weekly Wage", "Haftalık Maaş"),
        ("Squad Status", "Kadro Durumu"),
        ("Squad Number", "Forma Numarası"),
        ("Traits (PPMs)", "Özellikler (PPM)"),
        ("Join Club Date", "Kulübe Katılma Tarihi"),
        ("Transfer Date", "Transfer Tarihi"),
        ("Previous Club", "Önceki Kulüp"),
        ("Senior Debut", "İlk Kıdemli Maç"),
        ("Team", "Takım"),
        ("Club", "Kulüp"),
        // ── Staff coaching / roles (FM Turkish) ───────────────────────────
        ("Defending", "Savunma"),
        ("Attacking", "Hücum"),
        ("Tactical", "Taktik"),
        ("Defensive Line", "Savunma Hattı"),
        ("Tempo", "Tempo"),
        ("Width", "Genişlik"),
        ("Directness", "Direktlik"),
        ("Motivation", "Motivasyon"),
        ("Level of Discipline", "Disiplin Seviyesi"),
        ("Judge Ability", "Yetenek Tespiti"),
        ("Judge Potential", "Potansiyel Tespiti"),
        ("Judge Staff", "Personel Tespiti"),
        ("Data Analysis", "Veri Analizi"),
        ("Sports Science", "Spor Bilimi"),
        ("Physiotherapy", "Fizyoterapi"),
        ("Negotiating", "Müzakere"),
        ("Mind Games", "Zihin Oyunları"),
        ("Tactical Knowledge", "Taktik Bilgisi"),
        ("Buying Players", "Oyuncu Transferi"),
        ("Important Matches", "Önemli Maçlar"),
        ("Youngsters", "Genç Oyuncular"),
        ("Business", "İş Bilgisi"),
        ("Interference", "Müdahale"),
        ("Patience", "Sabır"),
        ("Expectations", "Beklentiler"),
        ("Resources", "Kaynaklar"),
        ("Affluence", "Zenginlik"),
        ("Flexibility", "Esneklik"),
        ("Consistency", "Tutarlılık"),
        ("Dirtiness", "Kirli Oyun"),
        ("Injury Proneness", "Sakatlanma Eğilimi"),
        ("Versatility", "Çok Yönlülük"),
        ("Train Hardness", "Antrenman Yoğunluğu"),
        ("Manager", "Menajer"),
        ("Asst Manager", "Yardımcı Menajer"),
        ("Coach", "Antrenör"),
        ("GK Coach", "Kaleci Antrenörü"),
        ("Fitness Coach", "Kondisyon Antrenörü"),
        ("Set Piece Coach", "Duran Top Antrenörü"),
        ("Physio", "Fizyoterapist"),
        ("Scout", "Gözlemci"),
        ("Data Analyst", "Veri Analisti"),
        ("Sports Scientist", "Spor Bilimci"),
        ("Loan Manager", "Transfer Sorumlusu"),
        ("Head of Youth Dev", "Altyapı Sorumlusu"),
        ("Dir of Football", "Futbol Direktörü"),
        ("Technical Director", "Teknik Direktör"),
        ("Managing", "Yönetim"),
        ("People Mgmt", "İnsan Yönetimi"),
        ("Adapt. to Country", "Ülkeye Uyum"),
        // ── Club / facilities / finance (FM Turkish) ──────────────────────
        ("Stadium", "Stadyum"),
        ("Max Attendance", "En Yüksek Seyirci"),
        ("Min Attendance", "En Düşük Seyirci"),
        ("Avg Attendance", "Ortalama Seyirci"),
        ("Season Tickets", "Sezon Biletleri"),
        ("Avg Season Ticket", "Ortalama Sezon Bileti"),
        ("Avg Ticket Price", "Ortalama Bilet Fiyatı"),
        ("Training Ground", "Antrenman Tesisi"),
        ("Training", "Antrenman"),
        ("Youth Academy", "Altyapı Akademisi"),
        ("Youth Facilities", "Altyapı Tesisleri"),
        ("Youth Coaching", "Altyapı Antrenmanı"),
        ("Youth Recruitment", "Altyapı Oyuncu Toplama"),
        ("Youth Improvement", "Altyapı Gelişimi"),
        ("Balance", "Bakiye"),
        ("Transfer Budget", "Transfer Bütçesi"), // witnessed 2026-11-07 (finance screen)
        ("Transfer Remaining", "Kalan Transfer Bütçesi"),
        ("Wage Budget", "Maaş Bütçesi"), // witnessed 2026-11-07 (finance screen)
        ("Wage Spend", "Maaş Harcaması"), // witnessed 2026-11-07 ("Şimdiki Harcama")
        ("Next Min Budget", "Önümüzdeki Sezon Min. Bütçesi"),
        ("Owner Status", "Sahip Durumu"),
        ("Year Founded", "Kuruluş Yılı"),
        ("Gender", "Cinsiyet"),
        ("Casual %", "Günlük %"),
        ("Family %", "Aile %"),
        ("Core %", "Çekirdek %"),
        ("Hardcore %", "Sadık %"),
        ("Fair Weather %", "Hava Şartlarına Göre %"),
        // ── Morale words (FM Turkish Dynamics morale scale) ───────────────
        ("Fair", "İdare Eder"),
        ("Okay", "Orta"),
        ("Fairly Good", "Oldukça İyi"),
        ("Quite Good", "Gayet İyi"),
        ("Good", "İyi"),
        ("Really Good", "Gerçekten İyi"),
        ("Very Good", "Çok İyi"),
        ("Extremely Good", "Son Derece İyi"),
        ("Excellent", "Harika"),
        ("Superb", "Süper"),
        ("Exceptional", "Olağanüstü"),
        ("Perfect", "Mükemmel"),
    ];
    pairs.iter().copied().collect()
});

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(lang: Language) -> egui::Context {
        let ctx = egui::Context::default();
        set_language(&ctx, lang);
        ctx
    }

    #[test]
    fn english_passes_through_and_unknown_strings_are_safe() {
        let ctx = ctx_with(Language::English);
        assert_eq!(tr(&ctx, "Players"), "Players");
        let tr_ctx = ctx_with(Language::Turkish);
        assert_eq!(
            tr(&tr_ctx, "Some untranslated sentence"),
            "Some untranslated sentence"
        );
    }

    #[test]
    fn turkish_uses_fm_game_terms_for_witnessed_fields() {
        let ctx = ctx_with(Language::Turkish);
        assert_eq!(tr(&ctx, "Finishing"), "Bitiricilik");
        assert_eq!(tr(&ctx, "First Touch"), "İlk Kontrol");
        assert_eq!(tr(&ctx, "Dribbling"), "Dripling");
        assert_eq!(tr(&ctx, "Crossing"), "Orta Yapma");
        assert_eq!(tr(&ctx, "Passing"), "Pas");
        assert_eq!(tr(&ctx, "Aggression"), "Agresiflik");
        assert_eq!(tr(&ctx, "Anticipation"), "Önsezi");
        assert_eq!(tr(&ctx, "Natural Fitness"), "Vücut Zindeliği");
        assert_eq!(tr(&ctx, "Transfer Budget"), "Transfer Bütçesi");
        assert_eq!(tr(&ctx, "Players"), "Oyuncular");
    }

    #[test]
    fn every_fm24_schema_attribute_label_has_a_turkish_entry() {
        // The schema is the source of truth; every player attribute label
        // that can render must localize (or deliberately stay English).
        let schemas = crate::game_version::GameBuild::Fm24Epic.schemas();
        let skip = [
            // Internal / never user-facing or codes that stay as-is.
            "Array Index",
            "Club Entity",
            "Team UID",
            "Staff ID",
            "Entity",
            "Abbreviation 1",
            "Abbreviation 2",
            "Abbreviation 3",
            "Stand Type 1",
            "Stand Type 2",
            "Stand Type 3",
            "Stand Type 4",
            "Name First Sub",
            "Name Last Sub",
            "Name2 First Sub",
            "Name2 Last Sub",
            "Name (secondary)",
            "AMC",
            "AML",
            "AMR",
            "DC",
            "DL",
            "DM",
            "DR",
            "GK",
            "MC",
            "ML",
            "MR",
            "ST",
            "SW",
            "WBL",
            "WBR",
        ];
        let mut missing = Vec::new();
        for f in schemas
            .player_fields
            .iter()
            .chain(schemas.staff_fields)
            .chain(schemas.club_fields)
        {
            if f.offset.is_none() || skip.contains(&f.label) {
                continue;
            }
            if !TR_TABLE.contains_key(f.label) {
                missing.push(f.label);
            }
        }
        assert!(missing.is_empty(), "no Turkish entry for: {missing:?}");
    }

    #[test]
    fn wage_suffixes_follow_the_language() {
        use crate::settings::WagePeriod::*;
        assert_eq!(wage_suffix(Weekly, Language::English), "/wk");
        assert_eq!(wage_suffix(Weekly, Language::Turkish), "/hafta");
        assert_eq!(wage_suffix(Yearly, Language::Turkish), "/yıl");
    }
}
