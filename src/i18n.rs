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

pub fn set_currency(ctx: &egui::Context, currency: crate::settings::Currency) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("fm_currency"), currency));
}

pub fn currency(ctx: &egui::Context) -> crate::settings::Currency {
    ctx.data_mut(|d| d.get_temp::<crate::settings::Currency>(egui::Id::new("fm_currency")))
        .unwrap_or_default()
}

pub fn set_wage_period(ctx: &egui::Context, period: crate::settings::WagePeriod) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("fm_wage_period"), period));
}

pub fn wage_period(ctx: &egui::Context) -> crate::settings::WagePeriod {
    ctx.data_mut(|d| d.get_temp::<crate::settings::WagePeriod>(egui::Id::new("fm_wage_period")))
        .unwrap_or_default()
}

/// 3667769 → "3,667,769" (game-style thousands separators).
pub fn thousands(v: u64) -> String {
    let digits = v.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Internal GBP amount → display currency with separators and symbol,
/// e.g. 3,663,276 → "4,273,079 €".
pub fn format_money(gbp: u64, cur: crate::settings::Currency) -> String {
    let v = (gbp as f64 * cur.factor()).round() as u64;
    format!("{} {}", thousands(v), cur.symbol())
}

/// DragValue display formatter for money drags: internal GBP → display value.
pub fn money_drag_formatter(
    factor: f64,
    symbol: &str,
) -> impl Fn(f64, std::ops::RangeInclusive<usize>) -> String + 'static {
    let symbol = symbol.to_owned();
    move |v, _| {
        let shown = ((v * factor).round() as i64).max(0) as u64;
        format!("{} {}", thousands(shown), symbol)
    }
}

/// DragValue parser for money drags: display value → internal GBP.
pub fn money_drag_parser(factor: f64) -> impl Fn(&str) -> Option<f64> + 'static {
    move |s| {
        let cleaned: String = s
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        cleaned.parse::<f64>().ok().map(|v| v / factor)
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

/// Sort key implementing the Turkish alphabet order FM's Turkish UI uses:
/// a b c ç d e f g ğ h ı i j k l m n o ö p r s ş t u ü v y z.
/// Rust's default byte ordering puts the non-ASCII Turkish letters after z
/// (Çalışkanlık after Vizyon, Çeviklik last in the physical list); FM's own
/// screens sort them right after their base letter. Spaces and punctuation
/// are skipped so "Karar Alma" sorts before "Kararlılık".
pub fn turkish_sort_key(s: &str) -> Vec<(u8, u32)> {
    const ORDER: [(char, u8); 29] = [
        ('a', 0),
        ('b', 1),
        ('c', 2),
        ('ç', 3),
        ('d', 4),
        ('e', 5),
        ('f', 6),
        ('g', 7),
        ('ğ', 8),
        ('h', 9),
        ('ı', 10),
        ('i', 11),
        ('j', 12),
        ('k', 13),
        ('l', 14),
        ('m', 15),
        ('n', 16),
        ('o', 17),
        ('ö', 18),
        ('p', 19),
        ('r', 20),
        ('s', 21),
        ('ş', 22),
        ('t', 23),
        ('u', 24),
        ('ü', 25),
        ('v', 26),
        ('y', 27),
        ('z', 28),
    ];
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            if let Some((_, rank)) = ORDER.iter().find(|(l, _)| *l == lower) {
                return (*rank, 0);
            }
            // ASCII and other letters: keep a stable alphabetical feel with
            // unknown letters after the Turkish alphabet, digits after that.
            if lower.is_ascii_digit() {
                (40, lower as u32)
            } else {
                (30, lower as u32)
            }
        })
        .collect()
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
        ("GK Coaching", "Kaleci Antrenörlüğü"),
        ("Fitness Coaching", "Kondisyon Antrenörlüğü"),
        ("Roles (read-only)", "Görevler (salt okunur)"),
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
        ("Knowledge", "Bilgi Seviyesi"),
        ("Medical", "Medikal"),
        ("Reputation", "İtibar"),
        ("Staff Roles", "Görevler"),
        ("Tactical Style", "Taktik Stil"),
        ("Board Relations", "Yönetim İlişkileri"),
        ("Facilities", "Tesisler"),
        ("Positions", "Pozisyonlar"),
        ("Traits", "Oyun Karakteristiği"),
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
        ("Bank Balance", "Bakiye"),
        ("Player", "Oyuncu"),
        ("None", "Yok"),
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
        ("Adaptability", "Uyum"),
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
        ("Wage (£ p/w)", "Maaş"),
        ("Weekly Wage", "Haftalık Maaş"),
        ("Squad Status", "Forma Süresi"),
        ("Squad Number", "Forma Numarası"),
        ("Traits (PPMs)", "Oyun Karakteristiği (PPM)"),
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
        ("Judge Ability", "Oyuncu Yeteneği Değerlendirme"),
        ("Judge Potential", "Oyuncu Potansiyeli Değerlendirme"),
        (
            "Judge Staff",
            "Teknik Heyet Personeli Kalitesini Değerlendirme",
        ),
        ("Data Analysis", "Veri Analizi"),
        ("Sports Science", "Spor Bilimcisi"),
        ("Physiotherapy", "Fizyoterapi"),
        ("Negotiating", "Pazarlık Kabiliyeti"),
        ("Mind Games", "Zihin Oyunları"),
        ("Tactical Knowledge", "Taktiksel Bilgi Birikimi"),
        ("Buying Players", "Oyuncu Transferi"),
        ("Important Matches", "Önemli Maçlar"),
        ("Youngsters", "Gençlerle Çalışma"),
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
        ("People Mgmt", "Futbolcu İdare Yeteneği"),
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
        ("Balance", "Denge"),
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
        // ── App chrome, search, compare, Best XI ──────────────────────────
        ("Select for compare", "Karşılaştırma için seç"),
        ("Compare with selected", "Seçilenle karşılaştır"),
        ("Clear selection", "Seçimi temizle"),
        ("Players:", "Oyuncular:"),
        ("Staff:", "Personel:"),
        ("Clubs:", "Kulüpler:"),
        ("Connected to", "Bağlandı:"),
        ("Results", "Sonuçlar"),
        ("My clubs", "Kulüplerim"),
        ("Advanced", "Gelişmiş"),
        ("Advanced Search", "Gelişmiş Arama"),
        ("Role:", "Görev:"),
        ("OK", "Tamam"),
        ("Club ID:", "Kulüp UID:"),
        ("Exact current club UID", "Geçerli kulübün tam UID'si"),
        ("Inclusive ranges", "Kapsayıcı aralıklar"),
        ("Name or club UID", "İsim veya kulüp UID'si"),
        ("Managed clubs", "Yönetilen kulüpler"),
        (
            "Managed clubs unavailable. Select a club by name or UID.",
            "Yönetilen kulüpler mevcut değil. Kulüğü isim veya UID ile seç.",
        ),
        ("Tactic:", "Taktik:"),
        ("Room to grow", "Gelişim payı"),
        ("Minimum familiarity:", "En düşük uyum:"),
        ("Alternatives outside XI", "İlk 11 dışı alternatifler"),
        ("Show potential attributes", "Potansiyel özellikleri göster"),
        ("Current contract", "Mevcut Sözleşme"),
        ("Stats", "İstatistikler"),
        ("Status", "Durum"),
        ("Age:", "Yaş:"),
        ("Nation:", "Ülke:"),
        ("Intl Caps / Goals", "Milli Maç / Gol"),
        ("U21 Caps / Goals", "U21 Maç / Gol"),
        ("DOB", "Doğum"),
        ("Expiry", "Bitiş"),
        ("Wage (GBP/week)", "Maaş (£/hafta)"),
        ("Hidden", "Gizli"),
        ("Calendar", "Takvim"),
        (
            "Traits unavailable for this record or game version.",
            "Bu kayıt veya oyun sürümünde özellikler mevcut değil.",
        ),
        // ── Squad statuses (contract playing time; FM Turkish) ────────────
        ("Star Player", "Yıldız Oyuncu"),
        ("Important Player", "Önemli Oyuncu"),
        ("Regular Starter", "İlk 11 Oyuncusu"),
        ("Squad Player", "Rotasyon Oyuncusu"),
        ("Impact Sub", "Hamle Oyuncusu"),
        ("Fringe Player", "Marjinal Oyuncu"),
        ("Emergency Backup", "Joker Yedek"),
        ("Breakthrough Prospect", "Patlama Yapan Yetenek"),
        ("Future Prospect", "Geleceğin Yeteneği"),
        ("Hot Prospect", "Büyük Yetenek"),
        ("Youngster", "Genç Oyuncu"),
        ("Regular", "Düzenli Oyuncu"),
        ("First-Choice Goalkeeper", "İlk Tercih Kaleci"),
        ("Cup Goalkeeper", "Kupa Kalecisi"),
        ("Domestic Cup Goalkeeper", "Ulusal Kupa Kalecisi"),
        ("Continental Cup Goalkeeper", "Kıtasal Kupa Kalecisi"),
        ("Backup", "Yedek Oyuncu"),
        ("Surplus to Requirements", "İhtiyaç Yok"),
        // ── Player traits / PPMs (FM Turkish; user-verify against game) ───
        ("Ball Control", "Top Kontrolü"),
        ("Shooting", "Şut"),
        ("Movement", "Hareket"),
        ("Dribbling / Running", "Dripling / Koşu"),
        ("Runs With Ball Down Left", "Topla Soldan İlerler"),
        ("Runs With Ball Down Right", "Topla Sağdan İlerler"),
        ("Runs With Ball Through Centre", "Topla Ortadan İlerler"),
        ("Gets Into Opposition Area", "Rakip Ceza Sahasına Girer"),
        ("Moves Into Channels", "Kanalara Hareket Eder"),
        (
            "Gets Forward Whenever Possible",
            "Fırsat Buldukça Hücuma Katılır",
        ),
        ("Plays Short Simple Passes", "Kısa ve Basit Paslar Oynar"),
        ("Tries Killer Balls Often", "Sık Sık Öldürücü Pas Dener"),
        ("Shoots From Distance", "Uzaktan Şut Çeker"),
        ("Shoots With Power", "Güçlü Şut Çeker"),
        ("Places Shots", "Şutlarını Yerleştirir"),
        ("Curls Ball", "Topa Falso Verir"),
        ("Likes To Round Keeper", "Kaleciyi Geçmeyi Sever"),
        (
            "Tries To Break Offside Trap",
            "Ofsayt Tuzağını Bozmaya Çalışır",
        ),
        ("Uses Outside Of Foot", "Ayağının Dış Kısmını Kullanır"),
        ("Marks Opponent Tightly", "Rakibi Sıkı Markajlar"),
        ("Winds Up Opponents", "Rakipleri Hırpalar"),
        ("Argues With Officials", "Görevlilerle Tartışır"),
        ("Plays With Back To Goal", "Sırtı Kaleye Dönük Oynar"),
        ("Comes Deep To Get Ball", "Top Almak İçin Geri Gelir"),
        ("Plays One-Twos", "Duvar Pası Oynar"),
        ("Likes To Lob Keeper", "Kaleciyi Aşırma Vuruşu Sever"),
        ("Dictates Tempo", "Temboyu Yönetir"),
        ("Attempts Overhead Kicks", "Makas Vuruşu Dener"),
        ("Looks For Pass Over Score", "Gol Yerine Pas Arar"),
        ("Plays No Through Balls", "Ara Pas Oynamaz"),
        ("Stops Play", "Oyunu Durdurur"),
        (
            "Knocks Ball Past Opponent",
            "Topa Kayarak Hamle Yapmayı Sever",
        ),
        (
            "Shifts Ball To Right Before Dribble",
            "Dripling Öncesi Topu Sağa Kaydırır",
        ),
        (
            "Shifts Ball To Left Before Dribble",
            "Dripling Öncesi Topu Sola Kaydırır",
        ),
        ("Dwells On Ball", "Topla Oyalanır"),
        ("Late Runs Into Box", "Ceza Sahasına Geç Koşar"),
        (
            "Plays Way Out Of Trouble",
            "Sıkışmadan Topu Çıkararak Oynar",
        ),
        ("Stays Back At All Times", "Her Zaman Geride Kalır"),
        (
            "Avoids Using Weaker Foot",
            "Zayıf Ayağını Kullanmaktan Kaçınır",
        ),
        ("Tries Tricks", "Numaralar Dener"),
        ("Tries Long Range Free Kicks", "Uzaktan Serbest Vuruş Dener"),
        ("Dives Into Tackles", "Topa Sert Girer"),
        ("Does Not Dive Into Tackles", "Topa Sert Girmez"),
        ("Cuts Inside From Both Wings", "İki Kanattan İçeri Keser"),
        ("Hugs Line", "Çizgiye Yanaşır"),
        ("Gets Crowd Going", "Tribünleri Coşturur"),
        ("Tries First Time Shots", "İlk Vuruşta Şut Çeker"),
        ("Tries Long Range Passes", "Uzun Paslar Dener"),
        ("Likes Ball Played Into Feet", "Ayağa Top Almayı Sever"),
        (
            "Hits Free Kicks With Power",
            "Serbest Vuruşları Güçle Kullanır",
        ),
        ("Beats Opponent Repeatedly", "Rakibi Defalarca Geçer"),
        ("Switches Ball To Wings", "Topu Kanatlara Aktarır"),
        ("Will Retire At Top", "Kariyerini Zirvede Bitirir"),
        (
            "Will Play Football As Long As Possible",
            "Mümkün Olduğunca Uzun Oynar",
        ),
        ("Possesses Long Flat Throw", "Uzun Düz Taç Yeteneği Vardır"),
        ("Runs With Ball Often", "Topla Sık Sık İlerler"),
        ("Runs With Ball Rarely", "Topla Nadiren İlerler"),
        (
            "Attempts To Develop Weaker Foot",
            "Zayıf Ayağını Geliştirmeye Çalışır",
        ),
        ("Penalty Box Player", "Ceza Sahası Oyuncusu"),
        ("Long Throw For Counters", "Kontralar İçin Uzun Taç"),
        ("Avoids Long Shots", "Uzun Şutlardan Kaçınır"),
        ("Cuts Inside From Left Wing", "Sol Kanattan İçeri Keser"),
        ("Cuts Inside From Right Wing", "Sağ Kanattan İçeri Keser"),
        ("Crosses Early", "Erken Orta Yapar"),
        ("Brings Ball Out Of Defence", "Topu Savunmadan Çıkarır"),
        (
            "Demands To Take Free Kicks",
            "Serbest Vuruşları Kendisi Kullanmak İster",
        ),
        (
            "Demands To Take Penalties",
            "Penaltıları Kendisi Kullanmak İster",
        ),
        ("Plays Ball With Feet", "Topu Ayakla Oynatır"),
        ("Unknown PPM", "Bilinmeyen Özellik"),
        // ── Club panel / stadium / Best XI (third pass) ────────────────────
        ("Ground:", "Stadyum:"),
        ("Stadium (read-only)", "Stadyum (salt okunur)"),
        ("Supporter Profile", "Taraftar Profili"),
        ("Supporter Attributes", "Taraftar Özellikleri"),
        ("Attendance", "Seyirci"),
        ("Capacity", "Kapasite"),
        ("Seated", "Oturaklı"),
        ("Used", "Kullanılan"),
        ("All-Seater", "Tam Oturaklı"),
        ("Expansion", "Genişletme"),
        ("Pitch Length", "Saha Uzunluğu"),
        ("Pitch Width", "Saha Genişliği"),
        ("Official Name", "Resmî İsim"),
        ("Corporate", "Kurumsal"),
        ("Wage Budget (p/w)", "Maaş Bütçesi (haftalık)"),
        ("Wage Spend (p/w)", "Maaş Harcaması (haftalık)"),
        ("Finance data not available.", "Finans verisi mevcut değil."),
        (
            "Attendance sub-record not available.",
            "Seyirci alt kaydı mevcut değil.",
        ),
        ("Female", "Kadın"),
        (
            "Cached squad snapshot",
            "Önbellekteki kadro anlık görüntüsü",
        ),
        ("Current strength", "Güncel güç"),
        ("Growth headroom", "Gelişim payı"),
        ("positions filled", "pozisyon dolu"),
        ("players in scope", "kapsamdaki oyuncu"),
        ("Incomplete XI", "Eksik 11"),
        ("Position depth", "Pozisyon Derinliği"),
        ("No eligible player", "Uygun oyuncu yok"),
        ("No eligible alternatives", "Uygun alternatif yok"),
        ("First choice", "İlk Tercih"),
        ("Cover", "Yedek"),
        ("Starts at", "Başlangıç:"),
        ("Fit", "Form"),
        ("attributes", "öznitelik"),
        ("familiarity", "uyum"),
        ("room", "pay"),
        ("fit", "form"),
        ("Currency", "Para Birimi"),
        ("weekly", "haftalık"),
        ("monthly", "aylık"),
        ("yearly", "yıllık"),
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
    fn turkish_sort_key_orders_letters_by_the_turkish_alphabet() {
        let mut words = vec![
            "Vizyon",
            "Çalışkanlık",
            "Cesaret",
            "Zıplama",
            "Çeviklik",
            "Önsezi",
        ];
        words.sort_by_key(|w| turkish_sort_key(w));
        assert_eq!(
            words,
            vec![
                "Cesaret",
                "Çalışkanlık",
                "Çeviklik",
                "Önsezi",
                "Vizyon",
                "Zıplama"
            ]
        );
        // The merged FM24 technical list must render in the game's order.
        let mut technical = [
            "Uzun Taç",
            "Bitiricilik",
            "Serbest Vuruş Kullanma",
            "Kafa Vuruşu",
            "Korner",
            "Markaj",
            "Orta Yapma",
            "Pas",
            "Penaltı Kullanma",
            "Teknik",
            "Top Kapma",
            "Uzaktan Şut",
            "Dripling",
            "İlk Kontrol",
        ];
        technical.sort_by_key(|w| turkish_sort_key(w));
        assert_eq!(
            technical,
            [
                "Bitiricilik",
                "Dripling",
                "İlk Kontrol",
                "Kafa Vuruşu",
                "Korner",
                "Markaj",
                "Orta Yapma",
                "Pas",
                "Penaltı Kullanma",
                "Serbest Vuruş Kullanma",
                "Teknik",
                "Top Kapma",
                "Uzaktan Şut",
                "Uzun Taç",
            ]
        );
    }

    #[test]
    fn thousands_separators_match_the_game_style() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(3_667_769), "3,667,769");
        let eur = crate::settings::Currency::Eur;
        // Witnessed 2026-11-07: internal 4,100,645 renders 4,782,290 € in game.
        assert_eq!(format_money(4_100_645, eur), "4,782,290 €");
    }

    #[test]
    fn money_drag_round_trips_through_the_currency_factor() {
        let fmt = money_drag_formatter(1.166_228_8, "€");
        let parse = money_drag_parser(1.166_228_8);
        let shown = fmt(4_100_645.0, 0..=usize::MAX);
        assert_eq!(shown, "4,782,290 €");
        let back = parse(&shown).unwrap();
        assert!((back - 4_100_645.0).abs() < 0.51);
        let internal = 1_234_567.0;
        let shown = fmt(internal, 0..=usize::MAX);
        let back = parse(&shown).unwrap();
        assert!((back - internal).abs() < 0.51);
    }

    #[test]
    fn wage_suffixes_follow_the_language() {
        use crate::settings::WagePeriod::*;
        assert_eq!(wage_suffix(Weekly, Language::English), "/wk");
        assert_eq!(wage_suffix(Weekly, Language::Turkish), "/hafta");
        assert_eq!(wage_suffix(Yearly, Language::Turkish), "/yıl");
    }
}
