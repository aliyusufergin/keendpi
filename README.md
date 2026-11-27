# keendpi

**Keenetic router'lar için DPI engeli aşma aracı.** Discord, SNI/HTTP ile engellenen siteler ve "Güvenli İnternet" filtresi, ağdaki **bütün cihazlarda** (telefon, TV, konsol, misafirler), cihazlarda hiçbir ayar yapmadan açılır. VPN değildir; trafiğin başka bir sunucudan geçmez.

[English](README.en.md)

- Router'da çalışan küçük bir motordur (Rust, ~600 KB). Bir bağlantının yalnızca ilk birkaç paketine bakar, gerisi Keenetic'in hızlı yolundan (FASTNAT) geçer. Ölçümde ping'e en fazla +0,1 ms ekledi, hızın %98'inden fazlası korundu.
- Motor durursa ya da çökerse internet çalışmaya devam eder, yalnızca engel aşma devre dışı kalır.
- Router yeniden başlayınca ve internet bağlantısı kopup gelince kendiliğinden devam eder.
- Kaldırınca iz bırakmaz.

> Durum: Türksat Kablonet'te (PPPoE) Keenetic Hopper KN-3811 üzerinde test edildi. Diğer operatörler ve modeller test edilmedi; bkz. [Operatörler](#operatörler) ve [Modeller](#modeller).

## İçindekiler

- [Nasıl çalışır](#nasıl-çalışır)
- [Gereksinimler](#gereksinimler)
- [Kurulum](#kurulum)
- [Ayarlar](#ayarlar)
- [Liste modları](#liste-modları)
- [Kontrol komutu](#kontrol-komutu)
- [Güncelleme](#güncelleme)
- [Kaldırma](#kaldırma)
- [Sorun giderme](#sorun-giderme)
- [Operatörler](#operatörler)
- [Modeller](#modeller)
- [Sınırlar](#sınırlar)
- [Sorumluluk](#sorumluluk)
- [Geliştirme](#geliştirme)
- [Lisans](#lisans)

## Nasıl çalışır

Operatörün DPI cihazı, HTTPS bağlantısının ilk paketindeki (TLS ClientHello) site adına (SNI) bakar ve engelli bir site görürse bağlantıyı keser ya da engel sayfasına yönlendirir.

keendpi, engelli bir siteye giden bağlantının ilk paketinden hemen önce **sahte bir ilk paket** gönderir. Sahte pakette zararsız bir site adı (`www.google.com`) vardır ve TTL değeri, paket DPI cihazını geçtikten sonra yolda ölecek kadar düşüktür. DPI sahte paketi görür, bağlantıyı serbest bırakır; sahte paket sunucuya hiç ulaşmaz. Gerçek paket olduğu gibi gider. Düz HTTP (port 80) için de aynısını yapar.

- Hangi bağlantılara uygulanacağını listeler belirler (`auto` modda keendpi engelli siteleri kendisi de öğrenir).
- Sunucunun kaç adım uzakta olduğunu ölçer ve sahte paketin sunucuya asla ulaşmayacağı bir TTL seçer.
- Hiçbir veri router dışına gönderilmez; keendpi kimseye bağlanmaz, telemetri yoktur.

## Gereksinimler

1. **OPKG ve Entware.** Keenetic web arayüzünde, genel sistem ayarlarındaki bileşen seçeneklerinden şu bileşenler kurulu olmalı:
   - **OPKG paket yöneticisi**,
   - **Netfilter alt sistemi için çekirdek modülleri** (Netfilter subsystem kernel modules),
   - **DNS-over-TLS proxy** ve/veya **DNS-over-HTTPS proxy** (madde 2 için).

   Entware bir USB belleğe kurulu olmalı (help.keenetic.com'da "Entware" diye aratınca çıkan resmi rehber). Entware'e SSH ile bağlanabilmelisin (genellikle `ssh root@192.168.1.1 -p 222`).

2. **Şifreli DNS (DoH/DoT).** Operatörler engelli siteler için DNS'te de yanlış adres verir. Kablonet düz DNS sorgularını (1.1.1.1 ve 8.8.8.8 dahil) yolda yakalayıp engel sayfası adresini döndürür. keendpi DNS'e dokunmaz. Keenetic'te (menü adları sürüme göre değişebilir):
   - İnternet güvenliği / DNS yapılandırması bölümünden bir **DNS-over-TLS** veya **DNS-over-HTTPS** sunucusu ekle (ör. `1.1.1.1:853`, SNI `cloudflare-dns.com`),
   - düz DNS sunucularını kaldır; internet bağlantısının ayarlarında **sağlayıcının DNS'ini yok say** seçeneğini aç,
   - cihazlara DHCP ile ayrıca DNS sunucusu dağıtıyorsan (ör. `1.1.1.1`), onu da kaldır ki cihazlar router'ı kullansın.

   Aynısı Entware'den, Keenetic komut satırıyla (`ndmc`) da yapılabilir:

   ```sh
   ndmc -c "dns-proxy tls upstream 1.1.1.1 853 sni cloudflare-dns.com"
   ndmc -c "no ip name-server 1.1.1.1"        # varsa, her düz DNS sunucusu için
   ndmc -c "copy running-config startup-config"   # kalıcı kaydet
   ```

   Doğrulama: Entware'de `nslookup discord.com` komutu `195.175.254.2` gibi bir engel adresi değil, gerçek bir adres (`162.159.x.x`) vermeli.

3. Router'da başka bir DPI aracı (zapret, nfqws, ByeDPI vb.) **çalışmamalı**; aynı paketlerle uğraşırlar.

## Kurulum

Entware'e SSH ile bağlan (normal bir oturum aç; `ssh … 'komut'` biçiminde tek komut çalıştırma, o zaman PATH farklı olur) ve şunları çalıştır:

```sh
# 1. HTTPS ile indirebilen wget. Keenetic'in Entware kurulumunda genelde hazır gelir;
#    yalnızca yoksa kurulur (kuruluysa opkg install onu güncellerdi).
opkg update
opkg list-installed | grep -q '^wget-ssl ' || opkg install wget-ssl ca-certificates

# 2. keendpi paket kaynağını ekle
echo "src/gz keendpi https://aliyusufergin.github.io/keendpi/opkg" > /opt/etc/opkg/keendpi.conf

# 3. Kur
opkg update
opkg install keendpi
```

Kurulum sırasında `Package keendpi … has no valid architecture, ignoring` satırları görünebilir: paket kaynağında başka mimarilerin paketleri de var, opkg onları atlar. `keendpi stopped` ardından `keendpi: started` satırları da normaldir.

Kurulum keendpi'yi hemen başlatır; router her açıldığında da kendiliğinden başlar. Çalıştığını görmek için:

```sh
/opt/etc/init.d/S51keendpi check
```

Engelli siteler `direct: blocked …`, `with keendpi: ok` göstermeli (bkz. [Kontrol komutu](#kontrol-komutu)). Ardından telefonda Discord'u açmayı dene.

## Ayarlar

Ayar dosyası: `/opt/etc/keendpi/keendpi.conf`. Değiştirdikten sonra:

```sh
/opt/etc/init.d/S51keendpi restart
```

Liste dosyalarını değiştirdiğinde yeniden başlatmaya gerek yok; keendpi birkaç saniye içinde kendisi okur.

| Ayar | Varsayılan | Anlamı |
|---|---|---|
| `ENABLED` | `yes` | `no`: kurulu ama kapalı (açılışta ve güncellemede de başlamaz). |
| `MODE` | `auto` | `list`, `auto` veya `all`; bkz. [Liste modları](#liste-modları). |
| `LISTS` | `/opt/etc/keendpi/lists` | Alan adı listeleri. Klasör verilirse içindeki tüm `*.txt` dosyaları okunur. |
| `EXCLUDE` | `/opt/etc/keendpi/exclude.txt` | Hiçbir modda dokunulmayacak alan adları. |
| `AUTO_LIST` | `/opt/etc/keendpi/auto.txt` | `auto` modunun öğrendiği alan adları. |
| `HTTP` | `yes` | Düz HTTP (port 80) isteklerini de ele al. |
| `VERBOSE` | `no` | `yes`: her bağlantıyı sistem günlüğüne yaz (sorun giderme için; çok satır üretir). |
| `TTL` | `3` | Sahte paketin router'dan çıkarken TTL'i. Kablonet'te 2 ve 3 çalışır. Daha yakın sunucular için kendiliğinden düşürülür. |
| `FAKE_HOST` | `www.google.com` | Sahte paketteki site adı. |
| `WAN` | `auto` | İnternet arayüzü. `auto`: varsayılan rotanın arayüzü (PPPoE'de `ppp0`). Boşlukla ayrılmış birden çok ad yazılabilir. |
| `QUEUE`, `MARK`, `FORCE_MARK` | `200`, `0x40000000`, `0x20000000` | Teknik ayrıntı; yalnızca başka bir araçla çakışırsa değiştir. |

**Listeler.** Her satıra bir alan adı yazılır; bir alan adı alt alan adlarını da kapsar (`discord.com` → `cdn.discord.com`). `#` ile başlayan satırlar yorumdur.

- `lists/default.txt`: keendpi ile gelen liste (Discord ve bilinen engelli siteler). **Güncellemede değişir**, düzenleme.
- Kendi alan adların için `lists/` içinde yeni bir dosya aç, ör. `lists/user.txt`. Güncellemeler bu dosyaya dokunmaz.
- `exclude.txt`: keendpi'nin hiç dokunmaması gereken alan adları (bir site keendpi ile bozuluyorsa buraya yaz).
- `auto.txt`: öğrenilen alan adları. Yanlış öğrenilen bir satırı silmen yeterli.

## Liste modları

| Mod | Hangi bağlantılar |
|---|---|
| `list` | Yalnızca `LISTS` dosyalarındaki alan adları. |
| `auto` (varsayılan) | `LISTS` + keendpi'nin öğrendikleri. Engelli bir siteye ilk bağlantı engellenir, keendpi bunu fark edip siteyi `auto.txt`'ye ekler; **sayfayı yenilediğinde** açılır. |
| `all` | Bütün HTTPS/HTTP bağlantıları (EXCLUDE hariç). |

`EXCLUDE` her modda önceliklidir.

keendpi bir siteyi şu durumlarda engelli sayar:
- bağlantı, sunucunun cevap vermesine yetecek süreden çok önce sıfırlanırsa (DPI'nin sahte sıfırlaması: Kablonet'te ~1,6 ms, sunucudan gerçek cevap en az ~9 ms),
- HTTPS isteğine TLS yerine bir HTTP yönlendirmesi gelirse (Güvenli İnternet'in engel sayfası).

Sunucuların kendi sıfırlamaları engel sayılmaz.

## Kontrol komutu

```sh
/opt/etc/init.d/S51keendpi check                 # listedeki tüm alan adları
/opt/etc/init.d/S51keendpi check ornek.com …     # verdiğin alan adları
```

Her alan adı için router'dan iki HTTPS (port 443) denemesi yapar: **direct** (keendpi'siz) ve **with keendpi** (keendpi'yle, listede olmasa bile). Sonunda bir kontrol alan adı (`example.com`) vardır. Düz HTTP (port 80) engelleri bu komutla denenmez.

| Sonuç | Anlamı |
|---|---|
| `ok` | Sunucu cevap verdi. |
| `blocked (reset)` | Bağlantı sıfırlandı (DPI). |
| `blocked (block page)` | Engel sayfasına yönlendirildi (ör. Güvenli İnternet). |
| `blocked (closed)` / `blocked (no answer)` | Bağlantı kapatıldı / cevap gelmedi. |
| `no connection` | Sunucuya bağlanılamadı. DNS bir engel adresi veriyor olabilir (bkz. [Gereksinimler](#gereksinimler) madde 2) ya da adres IP ile engelli veya kapalı olabilir; keendpi bunu aşamaz. |
| `no DNS answer` | Alan adının adresi yok (ör. yalnızca alt alan adları kullanılıyor). |
| `engine not running` | keendpi çalışmıyor; `with keendpi` denenmedi. `S51keendpi start`. |
| `(not listed)` / `(excluded)` | keendpi normalde bu alan adına dokunmaz; `with keendpi` sütunu yine de denemenin sonucudur. |

`direct: blocked`, `with keendpi: ok` = engel var ve keendpi aşıyor.

## Güncelleme

```sh
opkg update
opkg upgrade keendpi
```

Ayar dosyan, kendi listelerin ve öğrenilenler korunur.

> `opkg install --force-reinstall keendpi` **kullanma**: opkg bunu kaldırıp kurmak olarak işler ve `/opt/etc/keendpi` silinir.

## Kaldırma

```sh
opkg remove --autoremove keendpi
```

Bu; keendpi'yi durdurur, kurallarını ve yüklediği çekirdek modülünü kaldırır, `/opt/etc/keendpi` klasörünü (ayarlar, listeler, öğrenilenler) ve paket kaynağı dosyasını (`/opt/etc/opkg/keendpi.conf`) siler, keendpi ile gelen `iptables` paketini de kaldırır. **Kendi listelerini saklamak istiyorsan önce yedekle.**

Router'daki DNS ayarlarına keendpi hiç dokunmaz; onları kendin değiştirdiysen öyle kalır.

## Sorun giderme

**Günlük.** keendpi, Keenetic'in sistem günlüğüne yazar (web arayüzündeki sistem günlüğü sayfası), ya da Entware'de:

```sh
ndmc -c "show log" | grep keendpi
```

**Durum.**

```sh
/opt/etc/init.d/S51keendpi status
iptables -t mangle -S | grep keendpi     # kurallar (iptables keendpi ile birlikte kurulur)
```

**Bir site açılmıyor.**
1. `/opt/etc/init.d/S51keendpi check site.com` çalıştır.
   - `direct: ok`: site engelli değil; sorun başka yerde.
   - `no connection`: DNS sorunu (madde 2).
   - `with keendpi: blocked …`: keendpi bu engeli aşamıyor. `TTL`'i 2, 4 veya 5 yapıp `restart` ve tekrar `check` dene. Sonuç değişmezse operatör farklı bir yöntem kullanıyor olabilir; lütfen [bir issue aç](https://github.com/aliyusufergin/keendpi/issues) ve `check` çıktısını, operatörünü ve denediğin `TTL` değerlerini ekle.
2. `auto` moddaysan sayfayı bir kez yenile; ilk deneme öğrenme içindir. Kendin de `lists/user.txt`'ye ekleyebilirsin.

**Bir site keendpi ile bozuldu.** Alan adını `exclude.txt`'ye yaz.

**keendpi'yi geçici olarak kapatmak.** `/opt/etc/init.d/S51keendpi stop` (açılışta yine başlar). Kalıcı olarak kapatmak için `ENABLED=no`.

**Ayrıntılı günlük.** `VERBOSE=yes` yapıp `restart`: keendpi her bağlantıda ne yaptığını (sahte paket gönderdi mi, hangi TTL ile, sıfırlama geldi mi) günlüğe yazar. İşin bitince `no` yap.

**Ayar hatası.** `keendpi validate` hatanın satırını söyler:

```sh
/opt/sbin/keendpi validate
```

## Operatörler

| Operatör | Durum | Not |
|---|---|---|
| Türksat Kablonet | **Test edildi** | Engel: sıfırlama ve Güvenli İnternet yönlendirmesi. `TTL=3` (2 de çalışır). |
| Türk Telekom | Test edilmedi | Kullanıcı raporlarına göre Kablonet'e benzer (sıfırlama). Önce varsayılanla dene, olmazsa `TTL` 2–5. |
| Vodafone | Test edilmedi | Kullanıcı raporlarına göre sıfırlama. Varsayılanla dene, olmazsa `TTL` 2–5. |
| Superonline | Test edilmedi | Farklı bir DPI cihazı kullandığı bildiriliyor; çalışmayabilir. |
| TurkNet | Test edilmedi | Bildirilen engeller yalnızca DNS'te; şifreli DNS tek başına yetebilir. |

Kendi operatöründe denediysen, sonucu (`check` çıktısı ve kullandığın `TTL`) bir issue ile paylaşırsan tablo güncellenir.

## Modeller

`opkg print-architecture` çıktısında hangi mimariyi gördüğüne bak:

| Entware mimarisi | Durum |
|---|---|
| `aarch64-3.10` | Test edildi (Keenetic Hopper KN-3811, KeeneticOS 5.1). |
| `mipsel-3.4` | Paket derleniyor, **test edilmedi**. |

Big-endian `mips` için paket yok.

Gereken çekirdek modülleri (`nfnetlink_queue`, `xt_NFQUEUE`, `xt_u32`) KN-3811'de Netfilter bileşeniyle geliyor; keendpi yüklü değillerse kendisi yükler, bulamazsa günlüğe yazar.

## Sınırlar

- **IP ile engellenen siteler** ve **hız kısıtlaması** aşılamaz; keendpi bir VPN değildir.
- **DNS** engellerini keendpi çözmez; şifreli DNS gerekir (madde 2).
- Yalnızca **IPv4**. QUIC (HTTP/3) ele alınmaz; Kablonet'te QUIC engellenmiyor.
- Site adı ilk paketin dışında kalan (büyük) ClientHello'larda keendpi adı göremediği için sahte paketi her durumda gönderir; bu bağlantılarda `EXCLUDE` uygulanamaz. Sahte paket sunucuya ulaşmadığından zararsızdır.
- Router'a ~3 ms'den yakın engelli sunucular `auto` modda öğrenilemez; onları listeye kendin ekle.
- Kaldırma, `/opt/etc/keendpi` altındaki her şeyi siler.

## Sorumluluk

keendpi, erişimi kısıtlanan içeriğe ulaşmak için kullanılabilecek bir ağ aracıdır. Kullanımının yasal sonuçları kullanıcıya aittir; yazılım "olduğu gibi", hiçbir garanti olmadan sunulur (bkz. lisanslar).

## Geliştirme

```sh
docker/build.sh test                                                  # birim testleri
docker/build.sh build --release --target aarch64-unknown-linux-musl   # router için ikili
tools/mkipk.sh                                                        # dist/keendpi_<sürüm>_aarch64-3.10.ipk
```

Sürüm yayınlama: [RELEASING.md](RELEASING.md).

Kod düzeni: `src/engine.rs` (paket işleme, öğrenme), `src/supervise.rs` (bekçi), `src/check.rs`, `files/rules.sh` (netfilter kuralları), `files/S51keendpi` (servis), `files/50-keendpi.sh` (NDM kancası).

## Lisans

MIT veya Apache-2.0, tercihine göre ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
