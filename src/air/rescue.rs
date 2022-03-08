use super::{Digest, are_equal, EvaluationResult, Felt, FieldElement, Rescue, HASH_CYCLE_LEN};
use core::ops::Range;
use core::convert::TryInto;
use winterfell::{
    math::{fields::f64::BaseElement as BaseElement, StarkField},
};

// Evil stuffs
const STATE_WIDTH: usize = 12;
const RATE_RANGE: Range<usize> = 4..12;
const RATE_WIDTH: usize = RATE_RANGE.end - RATE_RANGE.start;
const CAPACITY_RANGE: Range<usize> = 0..4;
const DIGEST_RANGE: Range<usize> = 4..8;
pub fn merge_evil(values: &[Digest; 2]) -> Digest {
    // initialize the state by copying the digest elements into the rate portion of the state
    // (8 total elements), and set the first capacity element to 8 (the number of elements to
    // be hashed).
    let mut state = [BaseElement::ZERO; STATE_WIDTH];
    state[RATE_RANGE].copy_from_slice(Digest::digests_as_elements(values));
    // state[CAPACITY_RANGE.start] = BaseElement::new(RATE_WIDTH as u64);
    state[CAPACITY_RANGE.start] = BaseElement::new(1 as u64);

    // apply the Rescue permutation and return the first four elements of the state
    apply_permutation(&mut state);
    Digest::new(state[DIGEST_RANGE].try_into().unwrap())
}
/// Applies Rescue-XLIX permutation to the provided state.
pub fn apply_permutation(state: &mut [BaseElement; STATE_WIDTH]) {
    // implementation is based on algorithm 3 from <https://eprint.iacr.org/2020/1143.pdf>
    // apply round function 7 times; this provides 128-bit security with 40% security margin
    for i in 0..NUM_ROUNDS {
        apply_round(state, i);
    }
}
/// Rescue-XLIX round function.
#[inline(always)]
pub fn apply_round(state: &mut [BaseElement; STATE_WIDTH], round: usize) {
    // apply first half of Rescue round
    apply_sbox(state);
    apply_mds(state);
    add_constants(state, &ARK1[round]);

    // apply second half of Rescue round
    apply_inv_sbox(state);
    apply_mds(state);
    add_constants(state, &ARK2[round]);
}
#[inline(always)]
fn add_constants(state: &mut [BaseElement; STATE_WIDTH], ark: &[BaseElement; STATE_WIDTH]) {
    state.iter_mut().zip(ark).for_each(|(s, &k)| *s += k);
}
#[inline(always)]
fn exp_acc<B: StarkField, const N: usize, const M: usize>(base: [B; N], tail: [B; N]) -> [B; N] {
    let mut result = base;
    for _ in 0..M {
        result.iter_mut().for_each(|r| *r = r.square());
    }
    result.iter_mut().zip(tail).for_each(|(r, t)| *r *= t);
    result
}

////////////////////////////////

// RESCUE ROUND CONSTRAINTS
// ================================================================================================

/// when flag = 1, enforces constraints for a single round of Rescue hash functions
pub fn enforce_round<E: FieldElement + From<Felt>>(
    result: &mut [E],
    current: &[E],
    next: &[E],
    ark: &[E],
    flag: E,
) {
    // compute the state that should result from applying the first half of Rescue round
    // to the current state of the computation
    let mut step1 = [E::ZERO; STATE_WIDTH];
    step1.copy_from_slice(current);
    apply_sbox(&mut step1);
    apply_mds(&mut step1);
    for i in 0..STATE_WIDTH {
        step1[i] += ark[i];
    }

    // compute the state that should result from applying the inverse for the second
    // half for Rescue round to the next step of the computation
    let mut step2 = [E::ZERO; STATE_WIDTH];
    step2.copy_from_slice(next);
    for i in 0..STATE_WIDTH {
        step2[i] -= ark[STATE_WIDTH + i];
    }
    apply_inv_mds(&mut step2);
    apply_sbox(&mut step2);

    // make sure that the results are equal
    for i in 0..STATE_WIDTH {
        result.agg_constraint(i, flag, are_equal(step2[i], step1[i]));
    }
}

#[inline(always)]
fn apply_sbox<E: FieldElement + From<Felt>>(state: &mut [E; STATE_WIDTH]) {
    state.iter_mut().for_each(|v| {
        let t2 = v.square();
        let t4 = t2.square();
        *v *= t2 * t4;
    });
}

#[inline(always)]
fn apply_inv_sbox(state: &mut [BaseElement; STATE_WIDTH]) {
    // compute base^10540996611094048183 using 72 multiplications per array element
    // 10540996611094048183 = b1001001001001001001001001001000110110110110110110110110110110111

    // compute base^10
    let mut t1 = *state;
    t1.iter_mut().for_each(|t| *t = t.square());

    // compute base^100
    let mut t2 = t1;
    t2.iter_mut().for_each(|t| *t = t.square());

    // compute base^100100
    let t3 = exp_acc::<BaseElement, STATE_WIDTH, 3>(t2, t2);

    // compute base^100100100100
    let t4 = exp_acc::<BaseElement, STATE_WIDTH, 6>(t3, t3);

    // compute base^100100100100100100100100
    let t5 = exp_acc::<BaseElement, STATE_WIDTH, 12>(t4, t4);

    // compute base^100100100100100100100100100100
    let t6 = exp_acc::<BaseElement, STATE_WIDTH, 6>(t5, t3);

    // compute base^1001001001001001001001001001000100100100100100100100100100100
    let t7 = exp_acc::<BaseElement, STATE_WIDTH, 31>(t6, t6);

    // compute base^1001001001001001001001001001000110110110110110110110110110110111
    for (i, s) in state.iter_mut().enumerate() {
        let a = (t7[i].square() * t6[i]).square().square();
        let b = t1[i] * t2[i] * *s;
        *s = a * b;
    }
}

#[inline(always)]
fn apply_mds<E: FieldElement + From<Felt>>(state: &mut [E; STATE_WIDTH]) {
    let mut result = [E::ZERO; STATE_WIDTH];
    result.iter_mut().zip(MDS).for_each(|(r, mds_row)| {
        state.iter().zip(mds_row).for_each(|(&s, m)| {
            *r += E::from(m) * s;
        });
    });
    *state = result
}

#[inline(always)]
fn apply_inv_mds<E: FieldElement + From<Felt>>(state: &mut [E; STATE_WIDTH]) {
    let mut result = [E::ZERO; STATE_WIDTH];
    result.iter_mut().zip(INV_MDS).for_each(|(r, mds_row)| {
        state.iter().zip(mds_row).for_each(|(&s, m)| {
            *r += E::from(m) * s;
        });
    });
    *state = result
}

// ROUND CONSTANTS
// ================================================================================================

/// Returns Rescue round constants arranged in column-major form.
pub fn get_round_constants() -> Vec<Vec<Felt>> {
    let mut constants = Vec::new();
    for _ in 0..(STATE_WIDTH * 2) {
        constants.push(vec![Felt::ZERO; HASH_CYCLE_LEN]);
    }

    #[allow(clippy::needless_range_loop)]
    for i in 0..HASH_CYCLE_LEN - 1 {
        for j in 0..STATE_WIDTH {
            constants[j][i] = ARK1[i][j];
            constants[j + STATE_WIDTH][i] = ARK2[i][j];
        }
    }

    constants
}

// RESCUE CONSTANTS
// ================================================================================================

// const STATE_WIDTH: usize = Rescue::STATE_WIDTH;
const NUM_ROUNDS: usize = Rescue::NUM_ROUNDS;

/// Rescue MDS matrix
/// Computed using algorithm 4 from <https://eprint.iacr.org/2020/1143.pdf>
const MDS: [[Felt; STATE_WIDTH]; STATE_WIDTH] = [
    [
        Felt::new(2108866337646019936),
        Felt::new(11223275256334781131),
        Felt::new(2318414738826783588),
        Felt::new(11240468238955543594),
        Felt::new(8007389560317667115),
        Felt::new(11080831380224887131),
        Felt::new(3922954383102346493),
        Felt::new(17194066286743901609),
        Felt::new(152620255842323114),
        Felt::new(7203302445933022224),
        Felt::new(17781531460838764471),
        Felt::new(2306881200),
    ],
    [
        Felt::new(3368836954250922620),
        Felt::new(5531382716338105518),
        Felt::new(7747104620279034727),
        Felt::new(14164487169476525880),
        Felt::new(4653455932372793639),
        Felt::new(5504123103633670518),
        Felt::new(3376629427948045767),
        Felt::new(1687083899297674997),
        Felt::new(8324288417826065247),
        Felt::new(17651364087632826504),
        Felt::new(15568475755679636039),
        Felt::new(4656488262337620150),
    ],
    [
        Felt::new(2560535215714666606),
        Felt::new(10793518538122219186),
        Felt::new(408467828146985886),
        Felt::new(13894393744319723897),
        Felt::new(17856013635663093677),
        Felt::new(14510101432365346218),
        Felt::new(12175743201430386993),
        Felt::new(12012700097100374591),
        Felt::new(976880602086740182),
        Felt::new(3187015135043748111),
        Felt::new(4630899319883688283),
        Felt::new(17674195666610532297),
    ],
    [
        Felt::new(10940635879119829731),
        Felt::new(9126204055164541072),
        Felt::new(13441880452578323624),
        Felt::new(13828699194559433302),
        Felt::new(6245685172712904082),
        Felt::new(3117562785727957263),
        Felt::new(17389107632996288753),
        Felt::new(3643151412418457029),
        Felt::new(10484080975961167028),
        Felt::new(4066673631745731889),
        Felt::new(8847974898748751041),
        Felt::new(9548808324754121113),
    ],
    [
        Felt::new(15656099696515372126),
        Felt::new(309741777966979967),
        Felt::new(16075523529922094036),
        Felt::new(5384192144218250710),
        Felt::new(15171244241641106028),
        Felt::new(6660319859038124593),
        Felt::new(6595450094003204814),
        Felt::new(15330207556174961057),
        Felt::new(2687301105226976975),
        Felt::new(15907414358067140389),
        Felt::new(2767130804164179683),
        Felt::new(8135839249549115549),
    ],
    [
        Felt::new(14687393836444508153),
        Felt::new(8122848807512458890),
        Felt::new(16998154830503301252),
        Felt::new(2904046703764323264),
        Felt::new(11170142989407566484),
        Felt::new(5448553946207765015),
        Felt::new(9766047029091333225),
        Felt::new(3852354853341479440),
        Felt::new(14577128274897891003),
        Felt::new(11994931371916133447),
        Felt::new(8299269445020599466),
        Felt::new(2859592328380146288),
    ],
    [
        Felt::new(4920761474064525703),
        Felt::new(13379538658122003618),
        Felt::new(3169184545474588182),
        Felt::new(15753261541491539618),
        Felt::new(622292315133191494),
        Felt::new(14052907820095169428),
        Felt::new(5159844729950547044),
        Felt::new(17439978194716087321),
        Felt::new(9945483003842285313),
        Felt::new(13647273880020281344),
        Felt::new(14750994260825376),
        Felt::new(12575187259316461486),
    ],
    [
        Felt::new(3371852905554824605),
        Felt::new(8886257005679683950),
        Felt::new(15677115160380392279),
        Felt::new(13242906482047961505),
        Felt::new(12149996307978507817),
        Felt::new(1427861135554592284),
        Felt::new(4033726302273030373),
        Felt::new(14761176804905342155),
        Felt::new(11465247508084706095),
        Felt::new(12112647677590318112),
        Felt::new(17343938135425110721),
        Felt::new(14654483060427620352),
    ],
    [
        Felt::new(5421794552262605237),
        Felt::new(14201164512563303484),
        Felt::new(5290621264363227639),
        Felt::new(1020180205893205576),
        Felt::new(14311345105258400438),
        Felt::new(7828111500457301560),
        Felt::new(9436759291445548340),
        Felt::new(5716067521736967068),
        Felt::new(15357555109169671716),
        Felt::new(4131452666376493252),
        Felt::new(16785275933585465720),
        Felt::new(11180136753375315897),
    ],
    [
        Felt::new(10451661389735482801),
        Felt::new(12128852772276583847),
        Felt::new(10630876800354432923),
        Felt::new(6884824371838330777),
        Felt::new(16413552665026570512),
        Felt::new(13637837753341196082),
        Felt::new(2558124068257217718),
        Felt::new(4327919242598628564),
        Felt::new(4236040195908057312),
        Felt::new(2081029262044280559),
        Felt::new(2047510589162918469),
        Felt::new(6835491236529222042),
    ],
    [
        Felt::new(5675273097893923172),
        Felt::new(8120839782755215647),
        Felt::new(9856415804450870143),
        Felt::new(1960632704307471239),
        Felt::new(15279057263127523057),
        Felt::new(17999325337309257121),
        Felt::new(72970456904683065),
        Felt::new(8899624805082057509),
        Felt::new(16980481565524365258),
        Felt::new(6412696708929498357),
        Felt::new(13917768671775544479),
        Felt::new(5505378218427096880),
    ],
    [
        Felt::new(10318314766641004576),
        Felt::new(17320192463105632563),
        Felt::new(11540812969169097044),
        Felt::new(7270556942018024148),
        Felt::new(4755326086930560682),
        Felt::new(2193604418377108959),
        Felt::new(11681945506511803967),
        Felt::new(8000243866012209465),
        Felt::new(6746478642521594042),
        Felt::new(12096331252283646217),
        Felt::new(13208137848575217268),
        Felt::new(5548519654341606996),
    ],
];

const INV_MDS: [[Felt; STATE_WIDTH]; STATE_WIDTH] = [
    [
        Felt::new(1025714968950054217),
        Felt::new(2820417286206414279),
        Felt::new(4993698564949207576),
        Felt::new(12970218763715480197),
        Felt::new(15096702659601816313),
        Felt::new(5737881372597660297),
        Felt::new(13327263231927089804),
        Felt::new(4564252978131632277),
        Felt::new(16119054824480892382),
        Felt::new(6613927186172915989),
        Felt::new(6454498710731601655),
        Felt::new(2510089799608156620),
    ],
    [
        Felt::new(14311337779007263575),
        Felt::new(10306799626523962951),
        Felt::new(7776331823117795156),
        Felt::new(4922212921326569206),
        Felt::new(8669179866856828412),
        Felt::new(936244772485171410),
        Felt::new(4077406078785759791),
        Felt::new(2938383611938168107),
        Felt::new(16650590241171797614),
        Felt::new(16578411244849432284),
        Felt::new(17600191004694808340),
        Felt::new(5913375445729949081),
    ],
    [
        Felt::new(13640353831792923980),
        Felt::new(1583879644687006251),
        Felt::new(17678309436940389401),
        Felt::new(6793918274289159258),
        Felt::new(3594897835134355282),
        Felt::new(2158539885379341689),
        Felt::new(12473871986506720374),
        Felt::new(14874332242561185932),
        Felt::new(16402478875851979683),
        Felt::new(9893468322166516227),
        Felt::new(8142413325661539529),
        Felt::new(3444000755516388321),
    ],
    [
        Felt::new(14009777257506018221),
        Felt::new(18218829733847178457),
        Felt::new(11151899210182873569),
        Felt::new(14653120475631972171),
        Felt::new(9591156713922565586),
        Felt::new(16622517275046324812),
        Felt::new(3958136700677573712),
        Felt::new(2193274161734965529),
        Felt::new(15125079516929063010),
        Felt::new(3648852869044193741),
        Felt::new(4405494440143722315),
        Felt::new(15549070131235639125),
    ],
    [
        Felt::new(14324333194410783741),
        Felt::new(12565645879378458115),
        Felt::new(4028590290335558535),
        Felt::new(17936155181893467294),
        Felt::new(1833939650657097992),
        Felt::new(14310984655970610026),
        Felt::new(4701042357351086687),
        Felt::new(1226379890265418475),
        Felt::new(2550212856624409740),
        Felt::new(5670703442709406167),
        Felt::new(3281485106506301394),
        Felt::new(9804247840970323440),
    ],
    [
        Felt::new(7778523590474814059),
        Felt::new(7154630063229321501),
        Felt::new(17790326505487126055),
        Felt::new(3160574440608126866),
        Felt::new(7292349907185131376),
        Felt::new(1916491575080831825),
        Felt::new(11523142515674812675),
        Felt::new(2162357063341827157),
        Felt::new(6650415936886875699),
        Felt::new(11522955632464608509),
        Felt::new(16740856792338897018),
        Felt::new(16987840393715133187),
    ],
    [
        Felt::new(14499296811525152023),
        Felt::new(118549270069446537),
        Felt::new(3041471724857448013),
        Felt::new(3827228106225598612),
        Felt::new(2081369067662751050),
        Felt::new(15406142490454329462),
        Felt::new(8943531526276617760),
        Felt::new(3545513411057560337),
        Felt::new(11433277564645295966),
        Felt::new(9558995950666358829),
        Felt::new(7443251815414752292),
        Felt::new(12335092608217610725),
    ],
    [
        Felt::new(184304165023253232),
        Felt::new(11596940249585433199),
        Felt::new(18170668175083122019),
        Felt::new(8318891703682569182),
        Felt::new(4387895409295967519),
        Felt::new(14599228871586336059),
        Felt::new(2861651216488619239),
        Felt::new(567601091253927304),
        Felt::new(10135289435539766316),
        Felt::new(14905738261734377063),
        Felt::new(3345637344934149303),
        Felt::new(3159874422865401171),
    ],
    [
        Felt::new(1134458872778032479),
        Felt::new(4102035717681749376),
        Felt::new(14030271225872148070),
        Felt::new(10312336662487337312),
        Felt::new(12938229830489392977),
        Felt::new(17758804398255988457),
        Felt::new(15482323580054918356),
        Felt::new(1010277923244261213),
        Felt::new(12904552397519353856),
        Felt::new(5073478003078459047),
        Felt::new(11514678194579805863),
        Felt::new(4419017610446058921),
    ],
    [
        Felt::new(2916054498252226520),
        Felt::new(9880379926449218161),
        Felt::new(15314650755395914465),
        Felt::new(8335514387550394159),
        Felt::new(8955267746483690029),
        Felt::new(16353914237438359160),
        Felt::new(4173425891602463552),
        Felt::new(14892581052359168234),
        Felt::new(17561678290843148035),
        Felt::new(7292975356887551984),
        Felt::new(18039512759118984712),
        Felt::new(5411253583520971237),
    ],
    [
        Felt::new(9848042270158364544),
        Felt::new(809689769037458603),
        Felt::new(5884047526712050760),
        Felt::new(12956871945669043745),
        Felt::new(14265127496637532237),
        Felt::new(6211568220597222123),
        Felt::new(678544061771515015),
        Felt::new(16295989318674734123),
        Felt::new(11782767968925152203),
        Felt::new(1359397660819991739),
        Felt::new(16148400912425385689),
        Felt::new(14440017265059055146),
    ],
    [
        Felt::new(1634272668217219807),
        Felt::new(16290589064070324125),
        Felt::new(5311838222680798126),
        Felt::new(15044064140936894715),
        Felt::new(15775025788428030421),
        Felt::new(12586374713559327349),
        Felt::new(8118943473454062014),
        Felt::new(13223746794660766349),
        Felt::new(13059674280609257192),
        Felt::new(16605443174349648289),
        Felt::new(13586971219878687822),
        Felt::new(16337009014471658360),
    ],
];

/// Rescue round constants;
/// computed using algorithm 5 from <https://eprint.iacr.org/2020/1143.pdf>
///
/// The constants are broken up into two arrays ARK1 and ARK2; ARK1 contains the constants for the
/// first half of Rescue round, and ARK2 contains constants for the second half of Rescue round.
const ARK1: [[Felt; STATE_WIDTH]; NUM_ROUNDS] = [
    [
        Felt::new(13917550007135091859),
        Felt::new(16002276252647722320),
        Felt::new(4729924423368391595),
        Felt::new(10059693067827680263),
        Felt::new(9804807372516189948),
        Felt::new(15666751576116384237),
        Felt::new(10150587679474953119),
        Felt::new(13627942357577414247),
        Felt::new(2323786301545403792),
        Felt::new(615170742765998613),
        Felt::new(8870655212817778103),
        Felt::new(10534167191270683080),
    ],
    [
        Felt::new(14572151513649018290),
        Felt::new(9445470642301863087),
        Felt::new(6565801926598404534),
        Felt::new(12667566692985038975),
        Felt::new(7193782419267459720),
        Felt::new(11874811971940314298),
        Felt::new(17906868010477466257),
        Felt::new(1237247437760523561),
        Felt::new(6829882458376718831),
        Felt::new(2140011966759485221),
        Felt::new(1624379354686052121),
        Felt::new(50954653459374206),
    ],
    [
        Felt::new(16288075653722020941),
        Felt::new(13294924199301620952),
        Felt::new(13370596140726871456),
        Felt::new(611533288599636281),
        Felt::new(12865221627554828747),
        Felt::new(12269498015480242943),
        Felt::new(8230863118714645896),
        Felt::new(13466591048726906480),
        Felt::new(10176988631229240256),
        Felt::new(14951460136371189405),
        Felt::new(5882405912332577353),
        Felt::new(18125144098115032453),
    ],
    [
        Felt::new(6076976409066920174),
        Felt::new(7466617867456719866),
        Felt::new(5509452692963105675),
        Felt::new(14692460717212261752),
        Felt::new(12980373618703329746),
        Felt::new(1361187191725412610),
        Felt::new(6093955025012408881),
        Felt::new(5110883082899748359),
        Felt::new(8578179704817414083),
        Felt::new(9311749071195681469),
        Felt::new(16965242536774914613),
        Felt::new(5747454353875601040),
    ],
    [
        Felt::new(13684212076160345083),
        Felt::new(19445754899749561),
        Felt::new(16618768069125744845),
        Felt::new(278225951958825090),
        Felt::new(4997246680116830377),
        Felt::new(782614868534172852),
        Felt::new(16423767594935000044),
        Felt::new(9990984633405879434),
        Felt::new(16757120847103156641),
        Felt::new(2103861168279461168),
        Felt::new(16018697163142305052),
        Felt::new(6479823382130993799),
    ],
    [
        Felt::new(13957683526597936825),
        Felt::new(9702819874074407511),
        Felt::new(18357323897135139931),
        Felt::new(3029452444431245019),
        Felt::new(1809322684009991117),
        Felt::new(12459356450895788575),
        Felt::new(11985094908667810946),
        Felt::new(12868806590346066108),
        Felt::new(7872185587893926881),
        Felt::new(10694372443883124306),
        Felt::new(8644995046789277522),
        Felt::new(1422920069067375692),
    ],
    [
        Felt::new(17619517835351328008),
        Felt::new(6173683530634627901),
        Felt::new(15061027706054897896),
        Felt::new(4503753322633415655),
        Felt::new(11538516425871008333),
        Felt::new(12777459872202073891),
        Felt::new(17842814708228807409),
        Felt::new(13441695826912633916),
        Felt::new(5950710620243434509),
        Felt::new(17040450522225825296),
        Felt::new(8787650312632423701),
        Felt::new(7431110942091427450),
    ],
];

const ARK2: [[Felt; STATE_WIDTH]; NUM_ROUNDS] = [
    [
        Felt::new(7989257206380839449),
        Felt::new(8639509123020237648),
        Felt::new(6488561830509603695),
        Felt::new(5519169995467998761),
        Felt::new(2972173318556248829),
        Felt::new(14899875358187389787),
        Felt::new(14160104549881494022),
        Felt::new(5969738169680657501),
        Felt::new(5116050734813646528),
        Felt::new(12120002089437618419),
        Felt::new(17404470791907152876),
        Felt::new(2718166276419445724),
    ],
    [
        Felt::new(2485377440770793394),
        Felt::new(14358936485713564605),
        Felt::new(3327012975585973824),
        Felt::new(6001912612374303716),
        Felt::new(17419159457659073951),
        Felt::new(11810720562576658327),
        Felt::new(14802512641816370470),
        Felt::new(751963320628219432),
        Felt::new(9410455736958787393),
        Felt::new(16405548341306967018),
        Felt::new(6867376949398252373),
        Felt::new(13982182448213113532),
    ],
    [
        Felt::new(10436926105997283389),
        Felt::new(13237521312283579132),
        Felt::new(668335841375552722),
        Felt::new(2385521647573044240),
        Felt::new(3874694023045931809),
        Felt::new(12952434030222726182),
        Felt::new(1972984540857058687),
        Felt::new(14000313505684510403),
        Felt::new(976377933822676506),
        Felt::new(8407002393718726702),
        Felt::new(338785660775650958),
        Felt::new(4208211193539481671),
    ],
    [
        Felt::new(2284392243703840734),
        Felt::new(4500504737691218932),
        Felt::new(3976085877224857941),
        Felt::new(2603294837319327956),
        Felt::new(5760259105023371034),
        Felt::new(2911579958858769248),
        Felt::new(18415938932239013434),
        Felt::new(7063156700464743997),
        Felt::new(16626114991069403630),
        Felt::new(163485390956217960),
        Felt::new(11596043559919659130),
        Felt::new(2976841507452846995),
    ],
    [
        Felt::new(15090073748392700862),
        Felt::new(3496786927732034743),
        Felt::new(8646735362535504000),
        Felt::new(2460088694130347125),
        Felt::new(3944675034557577794),
        Felt::new(14781700518249159275),
        Felt::new(2857749437648203959),
        Felt::new(8505429584078195973),
        Felt::new(18008150643764164736),
        Felt::new(720176627102578275),
        Felt::new(7038653538629322181),
        Felt::new(8849746187975356582),
    ],
    [
        Felt::new(17427790390280348710),
        Felt::new(1159544160012040055),
        Felt::new(17946663256456930598),
        Felt::new(6338793524502945410),
        Felt::new(17715539080731926288),
        Felt::new(4208940652334891422),
        Felt::new(12386490721239135719),
        Felt::new(10010817080957769535),
        Felt::new(5566101162185411405),
        Felt::new(12520146553271266365),
        Felt::new(4972547404153988943),
        Felt::new(5597076522138709717),
    ],
    [
        Felt::new(18338863478027005376),
        Felt::new(115128380230345639),
        Felt::new(4427489889653730058),
        Felt::new(10890727269603281956),
        Felt::new(7094492770210294530),
        Felt::new(7345573238864544283),
        Felt::new(6834103517673002336),
        Felt::new(14002814950696095900),
        Felt::new(15939230865809555943),
        Felt::new(12717309295554119359),
        Felt::new(4130723396860574906),
        Felt::new(7706153020203677238),
    ],
];
