// Creates the admin and compliance Squads v4 multisigs (2-of-3, local member
// keys, no time lock) and records each vault PDA. The vault, not the multisig
// account, is what signs Corridor instructions, so it is what goes in Config.
// Safe to re-run: existing multisigs are left alone.
import * as squads from "@sqds/multisig";
import { confirm, connection, deployer, exists, explorer, key, updateRecord } from "./lib.mjs";

const { Permissions } = squads.types;
const THRESHOLD = 2;

async function createMultisig(role) {
  const createKey = key(`${role}-create-key`);
  const members = [1, 2, 3].map((i) => key(`${role}-member-${i}`).publicKey);
  const [multisigPda] = squads.getMultisigPda({ createKey: createKey.publicKey });
  const [vaultPda] = squads.getVaultPda({ multisigPda, index: 0 });

  if (await exists(multisigPda)) {
    console.log(`${role}: multisig already exists`);
  } else {
    const [programConfigPda] = squads.getProgramConfigPda({});
    const programConfig = await squads.accounts.ProgramConfig.fromAccountAddress(
      connection,
      programConfigPda,
    );
    const signature = await squads.rpc.multisigCreateV2({
      connection,
      treasury: programConfig.treasury,
      createKey,
      creator: deployer(),
      multisigPda,
      configAuthority: null, // autonomous: config changes need member approval
      timeLock: 0,
      threshold: THRESHOLD,
      members: members.map((member) => ({ key: member, permissions: Permissions.all() })),
      rentCollector: null,
    });
    await confirm(signature);
    console.log(`${role}: created ${explorer("tx", signature)}`);
  }

  console.log(`${role}: multisig ${multisigPda.toBase58()}, vault ${vaultPda.toBase58()}`);
  return {
    multisig: multisigPda.toBase58(),
    vault: vaultPda.toBase58(),
    threshold: `${THRESHOLD}-of-${members.length}`,
    members: members.map((member) => member.toBase58()),
  };
}

const admin = await createMultisig("admin");
const compliance = await createMultisig("compliance");
updateRecord({ squads: { admin, compliance } });
