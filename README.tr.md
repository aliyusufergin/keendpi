# keendpi

[![build](https://github.com/aliyusufergin/keendpi/actions/workflows/build.yml/badge.svg)](https://github.com/aliyusufergin/keendpi/actions/workflows/build.yml)
[![release](https://img.shields.io/github/v/release/aliyusufergin/keendpi)](https://github.com/aliyusufergin/keendpi/releases)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#lisans)

Keenetic router'lar için şeffaf DPI aşma aracı.

keendpi, Entware altında router'da çalışır ve SNI ile Host tabanlı engelleri ağdaki tüm cihazlar için aşar. Cihazlarda istemci, proxy ya da VPN gerekmez. Trafik de olağan yolundan çıkmaz.

[English](README.md)

## Özellikler

- **Ağ genelinde.** LAN'daki her cihaz, cihaz başına ayar gerekmeden yararlanır.
- **Düşük maliyet.** Her bağlantının yalnızca ilk paketlerine bakılır, gerisi Keenetic'in hızlı yolunda kalır. Referans cihazda medyan ping en fazla 0,1 ms arttı, hız %98'in üzerinde kaldı.
- **Fail-open.** Motor durur ya da takılırsa trafik olduğu gibi geçer. Takılan motoru bir bekçi (watchdog) yaklaşık bir saniye içinde yeniden başlatır.
- **Kendi kendine öğrenir.** `auto` modunda engelli alan adları sahte reset'lerden ve araya sokulan engelleme sayfalarından tespit edilip listeye eklenir.
- **Kalıcı.** Kurallar yeniden başlatmalardan, WAN yeniden bağlanmalarından ve firmware'in netfilter tablolarını yeniden yazmasından etkilenmez.
- **Temiz kaldırma.** Paketi kaldırmak eklediği her şeyi siler.

## Nasıl çalışır

DPI cihazları, bağlantının ilk veri paketinden sunucu adını okur: TLS ClientHello'daki SNI'dan ya da HTTP `Host` başlığından. Ad engel listesindeyse bağlantıyı sıfırlar veya araya bir yönlendirme sokar.

keendpi her bağlantının ilk isteğini NFQUEUE ile alır. Korunması gereken bağlantılarda gerçek istekten hemen önce bir yem istek gönderir:

- Yem, zararsız bir ad için (varsayılan `www.google.com`) bir ClientHello ya da HTTP isteğidir.
- TTL'i, DPI cihazını geçecek ama sunucuya varmadan ölecek kadar düşüktür. TTL, SYN-ACK'ten ölçülen hop mesafesiyle sınırlanır.

DPI cihazı bağlantıyı yeme göre sınıflandırır ve gerçek istek değişmeden geçer. keendpi asıl paketleri hiçbir zaman değiştirmez, geciktirmez ya da düşürmez.

TLS her TCP portunda, düz HTTP ise 80 portunda işlenir. keendpi kendisi hiçbir yere bağlanmaz ve telemetri toplamaz.

## Uyumluluk

### Router'lar

| Entware mimarisi (`opkg print-architecture`) | Durum |
|---|---|
| `aarch64-3.10` | Test edildi: Keenetic Hopper KN-3811, KeeneticOS 5.1 |
| `mipsel-3.4` | Derleniyor; test edilmedi |
| `mips-3.4` (big-endian) | Desteklenmiyor |

### Operatörler

| Operatör | Durum | Not |
|---|---|---|
| Türksat Kablonet | Test edildi | TCP reset'leri ve "Güvenli İnternet" yönlendirmeleri. `TTL=3` (2 de çalışır). |
| Türk Telekom | Test edilmedi | Kablonet gibi reset attığı bildiriliyor. Önce varsayılanlar, sonra `TTL` 2–5. |
| Vodafone | Test edilmedi | Reset attığı bildiriliyor. Önce varsayılanlar, sonra `TTL` 2–5. |
| Superonline | Test edilmedi | Farklı bir DPI üreticisi kullandığı bildiriliyor. |
| TurkNet | Test edilmedi | Yalnızca DNS ile engellediği bildiriliyor; şifreli DNS yetebilir. |

Başka operatör ve modellerden gelen sonuçlar memnuniyetle karşılanır; bkz. [Katkı](#katkı).

## Gereksinimler

1. **Firmware bileşenleri.** Şunlar kurulu olmalı: *OPKG paket yöneticisi*, *Netfilter alt sistemi çekirdek modülleri* ve *DNS-over-TLS proxy* ya da *DNS-over-HTTPS proxy*. Entware bir USB belleğe kurulu (help.keenetic.com'daki Entware rehberine bakın) ve SSH ile erişilebilir olmalı (varsayılan `ssh root@192.168.1.1 -p 222`).

2. **Şifreli DNS.** Operatörler DNS'e de müdahale eder; bazıları hangi sunucuya gönderilirse gönderilsin düz DNS sorgularını yolda yakalar. keendpi DNS'e karışmaz, bu yüzden router'da DoT ya da DoH ayarlayıp düz DNS sunucularını kaldırın. Entware'den:

   ```sh
   ndmc -c "dns-proxy tls upstream 1.1.1.1 853 sni cloudflare-dns.com"
   ndmc -c "no ip name-server 1.1.1.1"           # her düz DNS sunucusu için tekrarlayın
   ndmc -c "copy running-config startup-config"
   ```

   Ayrıca WAN bağlantısında *sağlayıcının DNS'ini yok say* seçeneğini açın ve DHCP'nin cihazlara ayrı bir DNS sunucusu vermediğinden emin olun. Router'da `nslookup discord.com` bir engelleme sayfası adresi değil, gerçek bir adres döndürmelidir.

3. Router'da **başka bir DPI aracı** (zapret, nfqws, ByeDPI, …) çalışmamalı.

## Kurulum

Aşağıdakileri Entware'de etkileşimli bir SSH oturumunda çalıştırın:

```sh
opkg update
opkg list-installed | grep -q '^wget-ssl ' || opkg install wget-ssl ca-certificates
echo "src/gz keendpi https://aliyusufergin.github.io/keendpi/opkg" > /opt/etc/opkg/keendpi.conf
opkg update
opkg install keendpi
```

`Package keendpi … has no valid architecture, ignoring` satırı normaldir: depoda birden çok mimarinin paketi bulunur.

Servis hemen ve her açılışta başlar. Çalıştığını şöyle doğrulayın:

```sh
/opt/etc/init.d/S51keendpi check
```

Paketler her [sürüme](https://github.com/aliyusufergin/keendpi/releases) de eklenir.

## Yapılandırma

Ayarlar `/opt/etc/keendpi/keendpi.conf` dosyasındadır. Değişiklikleri `/opt/etc/init.d/S51keendpi restart` ile uygulayın. Liste dosyaları birkaç saniye içinde kendiliğinden yeniden okunur.

| Anahtar | Varsayılan | Açıklama |
|---|---|---|
| `ENABLED` | `yes` | `no`: paket kurulu kalır ama çalışmaz. |
| `MODE` | `auto` | `list`, `auto` ya da `all` (aşağıya bakın). |
| `LISTS` | `/opt/etc/keendpi/lists` | Alan adı listesi dosyası ya da `*.txt` dosyalarının bulunduğu dizin. |
| `EXCLUDE` | `/opt/etc/keendpi/exclude.txt` | Hiçbir modda dokunulmayan alan adları. |
| `AUTO_LIST` | `/opt/etc/keendpi/auto.txt` | `auto` modunda öğrenilen alan adları. |
| `HTTP` | `yes` | 80 portundaki düz HTTP'yi de işle. |
| `TTL` | `3` | Yemin router'dan çıkarkenki TTL'i. Yakın sunucular için kendiliğinden düşürülür. |
| `FAKE_HOST` | `www.google.com` | Yemlerde kullanılan sunucu adı. |
| `WAN` | `auto` | WAN arayüz(ler)i. `auto`: varsayılan rotanın arayüzü. |
| `VERBOSE` | `no` | İşlenen her bağlantıyı günlüğe yaz. |
| `QUEUE`, `MARK`, `FORCE_MARK` | `200`, `0x40000000`, `0x20000000` | NFQUEUE numarası ve paket işareti bitleri. |

**Liste biçimi.** Her satırda bir alan adı. Alt alan adları da kapsanır; `#` yorum başlatır.

| Dosya | Amaç |
|---|---|
| `lists/default.txt` | Paketle gelen liste; güncellemede değiştirilir. |
| `lists/*.txt` | Kendi listeleriniz, örneğin `lists/user.txt`; güncellemede korunur. |
| `exclude.txt` | Dokunulmayacak alan adları. |
| `auto.txt` | Öğrenilen alan adları; unutturmak için satırı silin. |

### Modlar

| Mod | İşlenen bağlantılar |
|---|---|
| `list` | `LISTS` içindeki alan adları. |
| `auto` | `LISTS` ile öğrenilenler. Yeni bir alan adına ilk engellenen bağlantı onu öğretir, bir sonraki bağlantı açılır. |
| `all` | Tüm TLS ve HTTP bağlantıları. |

`EXCLUDE` her modda önceliklidir.

`auto` modunda bir bağlantı iki durumda engellenmiş sayılır:
- reset, ölçülen gidiş-dönüş süresinin yarısından kısa sürede gelir; yani sunucunun göndermiş olması imkânsızdır;
- ClientHello'ya TLS yerine düz HTTP ile cevap gelir; bu araya sokulmuş bir engelleme sayfasıdır.

## Kullanım

```sh
/opt/etc/init.d/S51keendpi {start|stop|restart|status}
/opt/etc/init.d/S51keendpi check [alan-adı ...]   # doğrudan ve keendpi ile karşılaştır
/opt/sbin/keendpi validate                        # yapılandırmayı denetle
```

`check`, her alan adı için router'dan iki HTTPS bağlantısı kurar: biri doğrudan, biri keendpi üzerinden. Sonra bir kontrol alan adını dener.

| Sonuç | Anlamı |
|---|---|
| `ok` | Sunucu cevap verdi. |
| `blocked (reset)` | Aradaki bir cihaz bağlantıyı sıfırladı. |
| `blocked (block page)` | Engelleme sayfasına yönlendirildi. |
| `blocked (closed)`, `blocked (no answer)` | Bağlantı kapandı ya da cevap gelmedi. |
| `no connection` | TCP bağlantısı kurulamadı: DNS engelleme adresi döndürdü, IP engelli ya da sunucu kapalı. |
| `no DNS answer` | Ad çözülmüyor. |
| `engine not running` | Servis durmuş. |

`direct: blocked` ile birlikte `with keendpi: ok` görmek, o alan adında keendpi'nin çalıştığını gösterir.

## Güncelleme ve kaldırma

```sh
opkg update && opkg upgrade keendpi    # ayarlar ve listeler korunur
opkg remove --autoremove keendpi       # kurallar, dosyalar, depo kaydı ve iptables kaldırılır
```

Kaldırma, listeleriniz dahil `/opt/etc/keendpi` dizinini siler. `opkg install --force-reinstall` kullanmayın: opkg bunu kaldırıp yeniden kurma olarak işler ve ayarları siler.

## Sorun giderme

- **Günlük:** `ndmc -c "show log" | grep keendpi` çalıştırın ya da web arayüzündeki sistem günlüğünü açın. Bağlantı başına ayrıntı için `VERBOSE=yes` yapın.
- **Kurallar:** `iptables -t mangle -S | grep keendpi` çalıştırın.
- **Bir site hâlâ açılmıyor:** o site için `check` çalıştırın.
  - `with keendpi: blocked`: `TTL` için 2, 4 ya da 5 deneyin.
  - `no connection`: engel SNI tabanlı değildir (DNS ya da IP).
  - `auto` modunda bir kez daha deneyin, çünkü ilk deneme öğrenmeye gider.
- **Bir site bozuluyor:** `exclude.txt` dosyasına ekleyin.

## Sınırlar

- IP tabanlı engeller ve hız kısıtlaması aşılamaz; keendpi bir VPN değildir.
- DNS engelleri için router'da şifreli DNS gerekir.
- Yalnızca IPv4 TCP işlenir. QUIC işlenmez.
- Bir ClientHello'nun SNI'ı ilk segmentin dışında kalırsa yem listelerden bağımsız olarak gönderilir, bu yüzden o bağlantıya `EXCLUDE` uygulanamaz. Bunun bir zararı yoktur.
- `auto` modunda yaklaşık 3 ms'den yakın sunuculardaki engeller, sunucunun kendi reset'lerinden ayırt edilemez. Bu alan adlarını listeye elle ekleyin.

## Kaynaktan derleme

Derleme Docker ile yapılır, bu yüzden yerelde Rust kurulu olması gerekmez.

```sh
docker/build.sh test
docker/build.sh build --release --target aarch64-unknown-linux-musl
tools/mkipk.sh            # → dist/keendpi_<sürüm>-1_aarch64-3.10.ipk
```

| Yol | İçerik |
|---|---|
| `src/engine.rs` | Paket işleme, yemler, öğrenme |
| `src/supervise.rs` | İşçi süreç denetimi ve bekçi |
| `src/check.rs` | `check` komutu |
| `files/rules.sh` | Netfilter kuralları |
| `files/S51keendpi` | Başlatma betiği |
| `files/50-keendpi.sh` | NDM netfilter kancası |

Sürüm çıkarma adımları [RELEASING.md](RELEASING.md) dosyasında, değişiklikler [CHANGELOG.md](CHANGELOG.md) dosyasındadır.

## Katkı

Hata bildirimleri ve pull request'ler memnuniyetle karşılanır. Bir site, operatör ya da router modeli bildirirken [bildirim şablonunu](https://github.com/aliyusufergin/keendpi/issues/new/choose) kullanın ve `check` çıktısını ekleyin.

## Sorumluluk reddi

keendpi bir ağ aracıdır. Size uygulanan yasalara uymak sizin sorumluluğunuzdadır. Yazılım "olduğu gibi", hiçbir garanti olmadan sunulur.

## Lisans

[MIT Lisansı](LICENSE-MIT) ya da [Apache Lisansı 2.0](LICENSE-APACHE) altında; seçim sizin.
